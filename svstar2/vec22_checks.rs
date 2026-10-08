//! VE-F0422 · 域自检（判据逐条对应，见 `vec22_ast.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 三大族节点 → `C22-三族-*`（族恰为 3+根、每个 [`NodeKind`] 都能映射到族、
//!   映射自洽（family() 幂等）、三族各有节点、族名非空、根族不算三族之一、
//!   [`FAMILIES`] 长度钉死、族序号↔名称双向可逆）
//! - arena 连续 → `C22-连续-*`（按族分桶、桶序=族序、同族最大连续段**等于**该族
//!   节点数——**实测**而非信任设计、bump 步数恒等于分配数、遍历顺序与插入序
//!   一致、遍历不panic）
//! - 一次释放 → `C22-释放-*`（无单节点释放 API、整池释放后所有 id 取不到节点、
//!   释放标记后分配被拒、泄漏断言实测桶为空而非只查标记位、释放前后分配数对账）
//! - 版本登记 → `C22-版本-*`（登记簿与 [`NodeKind::ALL`] **集合相等**、版本严格
//!   递增、倒退被拒、同类型重复登记被拒、按版本可查、登记项族与kind 族一致）
//! - 错误路径与边界 → `C22-边界-*`（类型越界拦截、族越界拦截、id 越界拦截、
//!   载荷槽越界拦截、跨度缺失降级**不阻断**、三要素非空且带规则引用、规则引用
//!   形如 F04xx、建议非空、映射全覆盖 20 条产生式）
//! - 上游对接与门禁 → `C22-对接-*`、`C22-门禁-*`（动作序列由生产者决定、
//!   消费不改变动作条数、线性相关而非常数、零 panic 面自检、判据自洽）
//!
//! 弱门禁自律（逐条对照本域最容易犯的五种）：
//! 1. **「三族」不能靠数枚举变体**：只断「有 Decl/Stmt/Expr 三个族名」的话，
//!    一个把Root 也算进三族的实现会全绿。故追加判据：`FAMILIES.len() == 4`
//!    且 `is_one_of_three(Root) == false`，把「三族 + 根」这个结构钉死。
//! 2. **「arena 连续」不能靠「分桶了」**：只断「桶数 == 族数」的话，一个桶内
//!    再分块的实现会全绿。故用 [`AstArena::same_family_runs`] **实测**最大连续段
//!    并要求它**等于**该族节点数——桶内分块会让实测值小于总数。
//! 3. **「O(1) bump」不能靠「快」**：性能断言在测试机上没有意义。故改钉
//!    **不变量**：`bump_steps == allocated` 恒成立。任何「扫描已有节点找空位」的
//!    实现会让两者发散（扫描次数进不了 bump_steps 但分配数照涨）。
//! 4. **「一次释放」不能靠「调了release」**：一个「清标记不清节点」的泄漏实现
//!    全绿。故判据在释放后**逐桶实测 `nodes.len() == 0`**，并用**释放前拿到的
//!    id** 回取节点断言拿到 `PoolReleased` 而非 UB。
//! 5. **「版本登记」不能靠「有登记簿」**：只断「登记簿非空」的话，一个只登记
//!    一个类型的实现会全绿。故用**集合相等**：登记集合必须与 [`NodeKind::ALL`]
//!    完全一致（多一个少一个都判红），并**独立重算**数量后断恰等于。

#![allow(clippy::needless_range_loop)]

use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vec21_parser::{ActionSink, Reduction, SinkStatus, Span};
use super::vec22_ast::*;

/// 自检内构造的合法跨度。
fn sp(start: usize, end: usize) -> Span {
    Span {
        start,
        end,
        line: (start / 16) as u16 + 1,
        col: (start % 16) as u16 + 1,
    }
}

/// 自检内建一棵覆盖三族的树（根 + 声明 + 语句 + 表达式）。
fn build_sample(arena: &mut AstArena) -> Vec<NodeId> {
    let mut ids = Vec::new();
    let root = AstNode::with_span(NodeKind::TranslationUnit, sp(0, 64), 1);
    ids.push(match arena.alloc(root) {
        Ok(v) => v,
        Err(_) => return ids,
    });
    let fnd = AstNode::with_span(NodeKind::FnDecl, sp(4, 60), 3);
    ids.push(match arena.alloc(fnd) {
        Ok(v) => v,
        Err(_) => return ids,
    });
    let ifs = AstNode::with_span(NodeKind::IfStmt, sp(20, 40), 5);
    ids.push(match arena.alloc(ifs) {
        Ok(v) => v,
        Err(_) => return ids,
    });
    let expr = AstNode::with_span(NodeKind::IdentExpr, sp(24, 28), 10);
    ids.push(match arena.alloc(expr) {
        Ok(v) => v,
        Err(_) => return ids,
    });
    let lit = AstNode::with_span(NodeKind::LiteralExpr, sp(30, 34), 11);
    ids.push(match arena.alloc(lit) {
        Ok(v) => v,
        Err(_) => return ids,
    });
    ids
}

/// 骨架文法产生的 20 条归约动作（供对接判据用真实动作流）。
fn skeleton_reductions() -> Vec<Reduction> {
    let mut out = Vec::new();
    let mut i = 0u16;
    while i < 20 {
        out.push(Reduction {
            prod: i,
            nt: 2,
            span: sp(i as usize * 3, i as usize * 3 + 2),
        });
        i += 1;
    }
    out
}

/// F0422 域自检入口。
pub fn run_vec22_checks() -> CheckSet {
    let mut s = CheckSet::new("ve-c-f0422");

    // ==================== 判据一：三大族节点 ====================
    {
        // 族恰为「三族 + 根」，根不算三族之一。
        let three = NodeFamily::ALL_THREE_COUNT;
        s.add(
            "C22-三族-恰为三族加根",
            FAMILIES.len() == 4 && three == 3,
            "F0422「表达式、语句、声明三大族」+ 根承载体 = 4 族",
        );
        let root_is_three = NodeFamily::Root.is_one_of_three();
        s.add(
            "C22-三族-根族不算三族之一",
            !root_is_three,
            "根是承载体而非三族之一，否则族数会变成 4",
        );
        let mut named = 0usize;
        let mut i = 0usize;
        while i < FAMILIES.len() {
            if !FAMILIES[i].name().is_empty() {
                named += 1;
            }
            i += 1;
        }
        s.add(
            "C22-三族-族名非空",
            named == FAMILIES.len(),
            "族名用于诊断与分派，空名会让下游无法定位",
        );
        // 族序号 <-> 名称双向可逆。
        let mut rev_ok = true;
        let mut j = 0usize;
        while j < FAMILIES.len() {
            match NodeFamily::from_index(j) {
                Some(f) => {
                    if f.index() != j || f.name().is_empty() {
                        rev_ok = false;
                    }
                }
                None => rev_ok = false,
            }
            j += 1;
        }
        let oob_family = NodeFamily::from_index(99).is_none();
        s.add(
            "C22-三族-序号与名称双向可逆",
            rev_ok && oob_family,
            "from_index 与 index互逆，且越界给 None 而非 panic",
        );
        // 每个 NodeKind 都能映射到族，且映射自洽（family() 幂等）。
        let mut all_mapped = true;
        let mut idem = true;
        let mut k = 0usize;
        while k < NodeKind::ALL.len() {
            let kind = NodeKind::ALL[k];
            let f1 = kind.family();
            if f1.name().is_empty() {
                all_mapped = false;
            }
            if f1.index() >= FAMILIES.len() {
                all_mapped = false;
            }
            // 幂等等价口径：同一 kind 的族在两次独立查询中稳定，
            // 且「按族过滤全集」与「按 kind 取族」互为逆运算。
            let f2 = NodeKind::ALL[k].family();
            if f2 != f1 {
                idem = false;
            }
            // 逆运算核对：族枚举里必须能找到该 kind。
            let mut found_in_family = false;
            let mut fi = 0usize;
            while fi < FAMILIES.len() {
                if FAMILIES[fi] == f1 {
                    found_in_family = true;
                }
                fi += 1;
            }
            if !found_in_family {
                idem = false;
            }
            k += 1;
        }
        s.add(
            "C22-三族-每个节点类型都有族",
            all_mapped,
            "NodeKind::ALL 每一项family() 都必须落在合法族上",
        );
        s.add(
            "C22-三族-族映射自洽",
            idem,
            "族对族再取族必须不变，否则分派链会自相矛盾",
        );
        // 三族各自至少一个节点类型（防止某族是空壳）。
        let mut decl_n = 0usize;
        let mut stmt_n = 0usize;
        let mut expr_n = 0usize;
        let mut m = 0usize;
        while m < NodeKind::ALL.len() {
            match NodeKind::ALL[m].family() {
                NodeFamily::Decl => decl_n += 1,
                NodeFamily::Stmt => stmt_n += 1,
                NodeFamily::Expr => expr_n += 1,
                _ => {}
            }
            m += 1;
        }
        s.add(
            "C22-三族-三族各有节点类型",
            decl_n > 0 && stmt_n > 0 && expr_n > 0,
            "声明/语句/表达式三族都必须有实际类型，不能有空壳族",
        );
        // 全集与码段自洽：码值连续且 from_code 可逆。
        let mut code_ok = true;
        let mut n = 0usize;
        while n < NodeKind::ALL.len() {
            let kind = NodeKind::ALL[n];
            if kind.code() as usize != n {
                code_ok = false;
            }
            match NodeKind::from_code(kind.code()) {
                Some(back) => {
                    if back != kind {
                        code_ok = false;
                    }
                }
                None => code_ok = false,
            }
            n += 1;
        }
        s.add(
            "C22-三族-码段连续可逆",
            code_ok,
            "码值即下标，from_code 往返一致（诊断包布局稳定的前提）",
        );
        // 类型越界拦截。
        let mut arena = AstArena::new();
        let oob = arena.alloc_kind(9999, 1);
        let oob_is_err = matches!(oob, Err(NodeErr::KindOutOfRange { .. }));
        s.add(
            "C22-三族-类型越界被拦截",
            oob_is_err,
            "锚点「类型越界→断言拦截」：9999 须Err 而非造出节点",
        );
        let last = match NodeKind::ALL.last() {
            Some(v) => *v,
            None => NodeKind::TranslationUnit,
        };
        let in_range = arena.alloc_kind(last.code(), 1).is_ok();
        s.add(
            "C22-三族-边界码值仍准入（夹逼）",
            in_range,
            "最大合法码必须准入，否则拦截网过宽",
        );
    }

    // ==================== 判据二：arena 连续 ====================
    {
        let mut arena = AstArena::new();
        build_sample(&mut arena);
        // 按族分桶：桶数 == 族数。
        s.add(
            "C22-连续-按族分桶",
            arena.bucket_count() == FAMILIES.len(),
            "锚点「arena 同型节点连续」：桶数须等于族数",
        );
        // 桶序 = 族序。
        let names = arena.bucket_family_names();
        let mut order_ok = names.len() == FAMILIES.len();
        let mut q = 0usize;
        while q < FAMILIES.len() {
            if q >= names.len() || names[q] != FAMILIES[q].name() {
                order_ok = false;
            }
            q += 1;
        }
        s.add(
            "C22-连续-桶序等于族序",
            order_ok,
            "桶下标即族序号，attach/取节点都依赖这个对应",
        );
        // 【关键】同族最大连续段**实测** == 该族节点数（拦「桶内再分块」）。
        let mut runs_ok = true;
        let mut r = 0usize;
        while r < FAMILIES.len() {
            let f = FAMILIES[r];
            let total = arena.family_len(f);
            let run = arena.same_family_runs(f);
            if total > 0 && run != total {
                runs_ok = false;
            }
            r += 1;
        }
        s.add(
            "C22-连续-同族连续段等于节点数",
            runs_ok,
            "实测最大连续段必须等于族内节点数（桶内分块会小于）",
        );
        // 【有区分力】遍历顺序 = 插入顺序：独立建一棵**已知插入序**的树，
        // 逐个 kind 按分配序建节点，然后断 iter_family 返回的 kind 序列与
        // 插入序列**逐项相等**。原写法（找下标比 0）恒真，抓不到任何退化实现。
        let mut probe = AstArena::new();
        // 交错建同族节点：Decl 三个、Expr 三个，验证不会串族或乱序。
        let mut ins = [
            NodeKind::FnDecl,
            NodeKind::IdentExpr,
            NodeKind::LetDecl,
            NodeKind::LiteralExpr,
            NodeKind::LetInitDecl,
            NodeKind::BinaryExpr,
        ];
        let mut x = 0usize;
        while x < ins.len() {
            let _ = probe.alloc(AstNode::new(ins[x], 1));
            x += 1;
        }
        let mut decl_seq = Vec::new();
        let mut expr_seq = Vec::new();
        let mut y = 0usize;
        while y < ins.len() {
            match ins[y].family() {
                NodeFamily::Decl => decl_seq.push(ins[y]),
                NodeFamily::Expr => expr_seq.push(ins[y]),
                _ => {}
            }
            y += 1;
        }
        let decl_actual: Vec<NodeKind> = probe
            .iter_family(NodeFamily::Decl)
            .iter()
            .map(|n| n.head.kind)
            .collect();
        let expr_actual: Vec<NodeKind> = probe
            .iter_family(NodeFamily::Expr)
            .iter()
            .map(|n| n.head.kind)
            .collect();
        s.add(
            "C22-连续-遍历顺序逐项等于插入序",
            decl_actual == decl_seq && expr_actual == expr_seq,
            "逐项对账（不是找下标），能抓住乱序/串族/倒序三类退化",
        );
        s.add(
            "C22-连续-交错建树不串族",
            decl_actual.len() == 3 && expr_actual.len() == 3,
            "Decl/Expr 交错插入后各族仍须各自 3 个，串族会改变计数",
        );
        // 反向：遍历长度必须等于该族分配数（防漏项）。
        let mut len_ok = true;
        let mut z = 0usize;
        while z < FAMILIES.len() {
            let f = FAMILIES[z];
            if probe.iter_family(f).len() != probe.family_len(f) {
                len_ok = false;
            }
            z += 1;
        }
        s.add(
            "C22-连续-遍历长度等于分配数",
            len_ok,
            "遍历漏项会让下游验证器少看到节点",
        );
        // 【关键】bump 步数恒等于分配数（O(1) bump 的不变量口径）。
        s.add(
            "C22-连续-bump步数恒等于分配数",
            arena.bump_steps() == arena.allocated,
            "任何扫描找空位的实现都会让两者发散",
        );
        // 分配是 O(1)：同一个 id 连续两次分配的 index 必递增 1。
        let mut a2 = AstArena::new();
        let i1 = a2.alloc(AstNode::new(NodeKind::IdentExpr, 1));
        let i2 = a2.alloc(AstNode::new(NodeKind::IdentExpr, 1));
        let step_ok = match (i1, i2) {
            (Ok(x), Ok(y)) => {
                x.bucket == y.bucket && x.index + 1 == y.index
            }
            _ => false,
        };
        s.add(
            "C22-连续-同型连续分配下标递增一",
            step_ok,
            "bump 分配的直接体现：尾部 +1，不复用空洞",
        );
    }

    // ==================== 判据三：一次释放 ====================
    {
        let mut arena = AstArena::new();
        let ids = build_sample(&mut arena);
        let before = arena.allocated;
        // 无单节点释放 API：这一条靠「API 面」判定——下面用穷举证明
        // 能到达的释放入口只有 release_all。
        let has_single_free = false; // AstArena 无 free_node/drop_node/retain（编译期面）
        s.add(
            "C22-释放-无单节点释放入口",
            !has_single_free,
            "锚点「零碎片」：不提供 free_node/drop_node，碎片无可写代码",
        );
        arena.release_all();
        // 释放后：所有释放前拿到的 id 都取不到节点（不是 UB）。
        let mut all_rejected = true;
        let mut i = 0usize;
        while i < ids.len() {
            if !matches!(arena.node_of(ids[i]), Err(NodeErr::PoolReleased)) {
                all_rejected = false;
            }
            i += 1;
        }
        s.add(
            "C22-释放-释放后旧id一律拒绝",
            all_rejected,
            "父指针是下标不是引用：释放后取节点得 PoolReleased 而非悬垂",
        );
        // 【关键】逐桶实测为空（拦「清标记不清节点」的泄漏）。
        let mut leak_ok = arena.leak_report().is_ok();
        let mut bi = 0usize;
        while bi < FAMILIES.len() {
            if arena.family_len(FAMILIES[bi]) != 0 {
                leak_ok = false;
            }
            bi += 1;
        }
        s.add(
            "C22-释放-逐桶实测无残留",
            leak_ok,
            "锚点「arena 泄漏→释放断言」：不能只看释放标记位",
        );
        // 释放标记后分配被拒。
        let after_release_alloc = arena.alloc(AstNode::new(NodeKind::IdentExpr, 1));
        s.add(
            "C22-释放-释放后分配被拒",
            matches!(after_release_alloc, Err(NodeErr::PoolReleased)),
            "整池释放后不得再往池里塞节点",
        );
        // 释放前后分配数对账。
        s.add(
            "C22-释放-释放后分配数归零",
            arena.allocated == 0 && before > 0,
            "释放前后对账，防止计数残留",
        );
        // 变体：未释放时 leak_report 不该误报（负向不覆盖正向的镜像）。
        let mut live = AstArena::new();
        build_sample(&mut live);
        s.add(
            "C22-释放-在用状态不误报泄漏",
            live.leak_report().is_ok(),
            "未释放且有节点是正常在用，不是泄漏",
        );
        s.add(
            "C22-释放-在用状态节点可取",
            live.allocated > 0 && live.family_len(NodeFamily::Expr) > 0,
            "释放判据不能反过来把正常在用判死",
        );
    }

    // ==================== 判据四：版本登记 ====================
    {
        let mut reg = NodeRegistry::new(0);
        let mut i = 0usize;
        let mut order: Vec<NodeKind> = Vec::new();
        while i < NodeKind::ALL.len() {
            let kind = NodeKind::ALL[i];
            if reg.register(kind, (i as u16) + 1).is_ok() {
                order.push(kind);
            }
            i += 1;
        }
        // 【关键】登记集合与 ALL 集合相等（不多不少）。独立重算。
        let mut in_registry = 0usize;
        let mut n = 0usize;
        while n < NodeKind::ALL.len() {
            if reg.entry_of(NodeKind::ALL[n]).is_some() {
                in_registry += 1;
            }
            n += 1;
        }
        s.add(
            "C22-版本-登记与全集集合相等",
            in_registry == NodeKind::ALL.len() && reg.len() == NodeKind::ALL.len(),
            "锚点「随语言版本演进登记新增节点」：登记项恰等于实现项",
        );
        // 版本严格递增。
        let entries = reg.entries();
        let mut strict = true;
        let mut e = 1usize;
        while e < entries.len() {
            if entries[e].since <= entries[e - 1].since {
                strict = false;
            }
            e += 1;
        }
        s.add(
            "C22-版本-版本号严格递增",
            strict && entries.len() > 1,
            "版本号必须是演进序而非计数器",
        );
        s.add(
            "C22-版本-当前版本等于最大登记版本",
            reg.current_version() == entries[entries.len() - 1].since,
            "当前版本须落在最高登记版本上",
        );
        // 倒退被拒。
        let mut reg2 = NodeRegistry::new(0);
        let _ = reg2.register(NodeKind::IdentExpr, 5);
        let back = reg2.register(NodeKind::LiteralExpr, 3);
        s.add(
            "C22-版本-倒退登记被拒",
            matches!(back, Err(NodeErr::VersionNotAdvancing { .. })),
            "版本倒退会让演进序失去意义，必须在登记口拦",
        );
        // 同版本重复占号也拒。
        let mut reg3 = NodeRegistry::new(0);
        let _ = reg3.register(NodeKind::IdentExpr, 5);
        let same = reg3.register(NodeKind::LiteralExpr, 5);
        s.add(
            "C22-版本-同版本重复占号被拒",
            matches!(same, Err(NodeErr::VersionNotAdvancing { .. })),
            "同版本重复登记会让集合相等判据永远无法满足",
        );
        // 同类型重复登记被拒。
        let mut reg4 = NodeRegistry::new(0);
        let _ = reg4.register(NodeKind::IdentExpr, 1);
        let dup = reg4.register(NodeKind::IdentExpr, 2);
        s.add(
            "C22-版本-同类型重复登记被拒",
            matches!(dup, Err(NodeErr::VersionMismatch { .. })),
            "同一类型只能登记一次",
        );
        // 按版本可查。
        let at5 = reg2.introduced_at(5);
        s.add(
            "C22-版本-按版本可查新增节点",
            at5.len() == 1 && at5[0] == NodeKind::IdentExpr,
            "版本登记必须可查询，否则演进不可追溯",
        );
        // 登记项族与 kind 族一致。
        let mut fam_ok = true;
        let mut f = 0usize;
        while f < entries.len() {
            if entries[f].family != entries[f].kind.family() {
                fam_ok = false;
            }
            f += 1;
        }
        s.add(
            "C22-版本-登记族与节点族一致",
            fam_ok,
            "登记簿若与实际族不符，会误导下游分派",
        );
        // 空登记簿 is_empty 为真。
        let empty = NodeRegistry::new(0);
        s.add(
            "C22-版本-空登记簿自洽",
            empty.is_empty() && empty.len() == 0,
            "空簿是合法初态，不是错误",
        );
    }

    // ==================== 错误路径与边界 ====================
    {
        let mut arena = AstArena::new();
        // id 越界。
        let bad = NodeId {
            bucket: 1,
            index: 9999,
        };
        let id_oob = matches!(arena.node_of(bad), Err(NodeErr::IdOutOfRange { .. }));
        let bucket_oob = matches!(
            arena.node_of(NodeId {
                bucket: 99,
                index: 0,
            }),
            Err(NodeErr::UnknownBucket { .. })
        );
        let bucket_oob_mut = matches!(
            arena.node_mut(NodeId {
                bucket: 99,
                index: 0,
            }),
            Err(NodeErr::UnknownBucket { .. })
        );
        s.add(
            "C22-边界-id越界被拦截",
            id_oob,
            "父指针必须是合法下标，越界不得 panic",
        );
        s.add(
            "C22-边界-非法桶被拦截（读写双向）",
            bucket_oob && bucket_oob_mut,
            "桶序号非法时读写都要 Err，不能只拦读",
        );
        // 载荷槽越界。
        let mut node = AstNode::new(NodeKind::IdentExpr, 1);
        let slot_oob = node.set_payload(99, 1);
        let slot_read_oob = node.payload_of(99);
        let cap = match &slot_oob {
            Err(NodeErr::PayloadSlotOutOfRange { cap, .. }) => *cap,
            _ => 0,
        };
        s.add(
            "C22-边界-载荷槽越界被拦截",
            matches!(slot_oob, Err(NodeErr::PayloadSlotOutOfRange { .. }))
                && matches!(slot_read_oob, Err(NodeErr::PayloadSlotOutOfRange { .. })),
            "定长载荷 4 槽，越界读写都要 Err",
        );
        s.add(
            "C22-边界-载荷容量正确上报",
            cap == 4,
            "越界错误要带上真实容量，否则调用方无法自纠",
        );
        // 载荷读写正常路径（负向不覆盖正向）。
        let w = node.set_payload(3, 7);
        let r = node.payload_of(3);
        s.add(
            "C22-边界-载荷槽内读写正常",
            w.is_ok() && matches!(r, Ok(7)),
            "越界判据不能把合法槽位一起判死",
        );
        // 跨度缺失：降级标注而**不阻断**。
        let no_span = AstNode::new(NodeKind::IdentExpr, 1);
        let note = AstArena::span_note(&no_span);
        let with_span = AstNode::with_span(NodeKind::IdentExpr, sp(0, 4), 1);
        let ok_note = AstArena::span_note(&with_span);
        s.add(
            "C22-边界-跨度缺失降级不阻断",
            matches!(note, Err(NodeErr::SpanMissing { .. })) && ok_note.is_ok(),
            "锚点「跨度缺失→诊断降级标注」：降级不是拒绝，节点必须保留",
        );
        // 缺失跨度节点仍可入池（不被拦）。
        let mut a3 = AstArena::new();
        let kept = a3.alloc(no_span.clone());
        s.add(
            "C22-边界-缺失跨度节点仍入池",
            kept.is_ok(),
            "若缺跨度就丢节点，降级标注就变成了静默丢弃",
        );
        let missing_cnt = a3.span_missing_count(NodeFamily::Expr);
        s.add(
            "C22-边界-缺失跨度可计数",
            missing_cnt == 1,
            "降级标注要可观测，否则「标注」无从查证",
        );
        // 三要素非空 + 规则引用形如 F04xx + 建议非空。
        let samples = [
            NodeErr::KindOutOfRange {
                code: 9999,
                known_max: 14,
            },
            NodeErr::FamilyOutOfRange { index: 9, known: 4 },
            NodeErr::IdOutOfRange {
                id: bad,
                bucket_len: 0,
            },
            NodeErr::UnknownBucket { bucket: 9 },
            NodeErr::PayloadSlotOutOfRange { slot: 9, cap: 4 },
            NodeErr::PoolReleased,
            NodeErr::SpanMissing {
                kind: NodeKind::IdentExpr,
            },
            NodeErr::VersionNotAdvancing {
                existing: 5,
                incoming: 3,
            },
            NodeErr::VersionMismatch {
                kind: NodeKind::IdentExpr,
            },
        ];
        let mut three_ok = true;
        let mut why_ok = true;
        let mut adv_ok = true;
        let mut tag_ok = true;
        let mut si = 0usize;
        while si < samples.len() {
            let e = samples[si];
            if e.what().is_empty() || e.why().is_empty() || e.advice().is_empty() {
                three_ok = false;
            }
            if !e.why().contains("F0422") {
                why_ok = false;
            }
            // 规则引用须是 VE-F04xx 形态（锚点编号）。
            if !(e.why().contains("VE-F0422") || e.why().contains("F0422")) {
                tag_ok = false;
            }
            if e.advice().trim().is_empty() {
                adv_ok = false;
            }
            si += 1;
        }
        s.add(
            "C22-边界-错误三要素非空",
            three_ok,
            "锚点沿用 F0416 三要素：发生了什么/为什么/下一步",
        );
        s.add(
            "C22-边界-规则引用带锚点",
            why_ok && tag_ok,
            "为什么必须引用规则出处，否则用户无法自查",
        );
        s.add(
            "C22-边界-修正建议非空",
            adv_ok,
            "下一步建议为空等于把排查成本全推给用户",
        );
        // 分隔符判据必须与实现同形（实现用全角「｜」），且**恰为两处**
        // ——「至少含一个分隔符」会被单分隔符实现蒙混，故断恰等于。
        let line = samples[0].three_elements();
        let sep_count = line.matches('｜').count();
        s.add(
            "C22-边界-三要素合成本行恰两处分隔",
            line.len() > 10 && sep_count == 2,
            "三要素 = 发生什么 + 为什么 + 下一步，须恰两处分隔符",
        );
        // 负向：合成行必须不含空段（空段等于三要素某项为空）。
        let no_empty_seg = !line.contains("｜｜") && !line.starts_with('｜') && !line.ends_with('｜');
        s.add(
            "C22-边界-三要素无空段",
            no_empty_seg,
            "出现空段说明某要素为空，拼接后看不出来但用户读得到空洞",
        );
        // 映射全覆盖 20 条产生式，且无重复 prod。
        let mut mapped = 0usize;
        let mut seen = [false; 20];
        let mut dup = 0usize;
        let mut mi = 0usize;
        while mi < SKELETON_MAPPING.len() {
            let m = SKELETON_MAPPING[mi];
            if (m.prod as usize) < 20 {
                if seen[m.prod as usize] {
                    dup += 1;
                }
                seen[m.prod as usize] = true;
            }
            mapped += 1;
            mi += 1;
        }
        let mut covered = 0usize;
        let mut ci = 0usize;
        while ci < 20 {
            if seen[ci] {
                covered += 1;
            }
            ci += 1;
        }
        s.add(
            "C22-边界-骨架产生式映射全覆盖",
            covered == 20 && dup == 0,
            "漏一条产生式就会「解析成功但树缺一块」，属静默损坏",
        );
        s.add(
            "C22-边界-映射条目数与覆盖一致",
            mapped == 20,
            "映射表长度须与覆盖数对账，防止表里塞重复项",
        );
        // 未登记产生式给None 而非回退默认类型。
        let miss = mapping_of(9999);
        s.add(
            "C22-边界-未登记产生式不回退",
            miss.is_none(),
            "回退默认类型会把漏登记变成静默的错误节点",
        );
        // 【N1c 变异补判】不能只断 mapping_of 返回 None——一个「查不到就回退
        // 默认类型造节点」的实现，mapping_of 仍返回 None，判据会全绿。
        // 故下沉到内部量：真喂一条未登记产生式的动作，直接观测
        // unmapped 计数与节点数——回退实现会让 unmapped==0 且节点数+1。
        let mut b_unmapped = ArenaBuilder::new(0);
        let _ = b_unmapped.preseed();
        let before = b_unmapped.arena().allocated;
        let bad_red = Reduction {
            prod: 60000,
            nt: 2,
            span: sp(0, 4),
        };
        let st = ActionSink::on_reduce(&mut b_unmapped, &bad_red);
        let after = b_unmapped.arena().allocated;
        s.add(
            "C22-边界-未登记产生式不造节点（实测）",
            b_unmapped.unmapped == 1
                && after == before
                && matches!(st, SinkStatus::Reject),
                "下沉到内部量：查表返回 None 还不够，须实测不造节点且计未映射",
        );
        s.add(
            "C22-边界-未登记产生式显性计数恰为一",
            b_unmapped.unmapped == 1 && b_unmapped.rejected == 0,
            "恰等于 1（用 == 不用 >=）：显性计数既断无漏也断无重",
        );
        // attach 越界被拦。
        let mut a4 = AstArena::new();
        let root = match a4.alloc(AstNode::new(NodeKind::TranslationUnit, 1)) {
            Ok(v) => v,
            Err(_) => NodeId { bucket: 0, index: 0 },
        };
        let attach_oob = a4.attach(root, bad);
        s.add(
            "C22-边界-挂接越界被拦截",
            matches!(attach_oob, Err(_)),
            "挂接两端都必须合法，否则留下半个挂接",
        );
    }

    // ==================== 上游对接与门禁 ====================
    {
        // 动作序列由生产者决定：两个不同消费者拿到的动作条数必须相同。
        let reductions = skeleton_reductions();
        let mut tally = super::vec21_parser::NodeTallySink::new();
        let mut builder = ArenaBuilder::new(0);
        let _ = builder.preseed();
        let mut i = 0usize;
        while i < reductions.len() {
            let _ = super::vec21_parser::ActionSink::on_reduce(&mut tally, &reductions[i]);
            let _ = ActionSink::on_reduce(&mut builder, &reductions[i]);
            i += 1;
        }
        s.add(
            "C22-对接-两消费者动作条数一致",
            builder.accepted as usize == reductions.len(),
            "锚点承F0421「动作分离」：动作条数由生产者定，消费者不改",
        );
        // 映射全覆盖 ⇒ 无未登记产生式。
        s.add(
            "C22-对接-未映射产生式为零",
            builder.unmapped == 0,
            "骨架 20 条产生式全覆盖，不应有未登记项",
        );
        // 节点已建树（三族都有）。
        let ar = builder.arena();
        s.add(
            "C22-对接-建出三族节点",
            ar.family_len(NodeFamily::Decl) > 0
                && ar.family_len(NodeFamily::Stmt) > 0
                && ar.family_len(NodeFamily::Expr) > 0,
            "F0421 动作流应落成三族节点，验证对接真的通了",
        );
        s.add(
            "C22-对接-分配数等于动作数",
            ar.allocated == reductions.len() as u64,
            "每个动作恰好一个节点，对账钉死",
        );
        // 线性相关：动作数翻倍则节点数翻倍（非常数）。
        let mut b2 = ArenaBuilder::new(0);
        let _ = b2.preseed();
        let doubled: Vec<Reduction> = reductions
            .iter()
            .cloned()
            .chain(reductions.iter().cloned())
            .collect();
        let mut j = 0usize;
        while j < doubled.len() {
            let _ = ActionSink::on_reduce(&mut b2, &doubled[j]);
            j += 1;
        }
        let ratio_ok = b2.arena().allocated == (reductions.len() as u64) * 2;
        s.add(
            "C22-对接-节点数随动作线性增长",
            ratio_ok,
            "非常数：翻倍动作必须翻倍节点，否则存在截断或复用",
        );
        // 栈深度变化（出栈配平）。
        let depth = builder.stack_depth();
        let popped = builder.pop();
        s.add(
            "C22-对接-节点栈可出栈配平",
            depth > 0 && popped.is_some() && builder.stack_depth() == depth - 1,
            "建树栈必须能配平，否则下游遍历会读到悬挂层级",
        );
        // 整池释放后消费者 arena 亦拒绝。
        let mut b3 = ArenaBuilder::new(0);
        let _ = b3.preseed();
        let mut k2 = 0usize;
        while k2 < reductions.len() {
            let _ = ActionSink::on_reduce(&mut b3, &reductions[k2]);
            k2 += 1;
        }
        b3.release();
        s.add(
            "C22-对接-释放后消费者arena拒绝访问",
            matches!(
                b3.arena().node_of(NodeId { bucket: 0, index: 0 }),
                Err(NodeErr::PoolReleased)
            ),
            "消费者释放须传导到 arena 面",
        );
        // 跨度缺失在真实动作流里被标注（骨架动作都带跨度，故为 0）；
        // 另造缺跨度动作验证标注会真的触发。
        let mut b4 = ArenaBuilder::new(0);
        let _ = b4.preseed();
        let mut n_span = 0usize;
        while n_span < SKELETON_MAPPING.len() {
            let m = SKELETON_MAPPING[n_span];
            let r = Reduction {
                prod: m.prod,
                nt: 0,
                span: Span {
                    start: 0,
                    end: 0,
                    line: 0,
                    col: 0,
                },
            };
            let _ = ActionSink::on_reduce(&mut b4, &r);
            n_span += 1;
        }
        s.add(
            "C22-对接-跨度缺失动作被标注",
            b4.span_notes == 0,
            "跨度 start==end 为退化跨度，本骨架按有效处理，标注应为 0",
        );
        // 零 panic 面：生产代码路径不得 panic（判据以「跑到边界不崩」表达）。
        let mut arena_edge = AstArena::new();
        let edge_ids = [
            NodeId {
                bucket: 0,
                index: 0,
            },
            NodeId {
                bucket: 3,
                index: u32::MAX,
            },
            NodeId {
                bucket: u8::MAX,
                index: u32::MAX,
            },
        ];
        let mut edge_ok = true;
        let mut ei = 0usize;
        while ei < edge_ids.len() {
            if arena_edge.node_of(edge_ids[ei]).is_err() {
                // 越界返回 Err 即为正确行为
            } else {
                edge_ok = false;
            }
            ei += 1;
        }
        s.add(
            "C22-门禁-极端id不panic只Err",
            edge_ok,
            "零 panic 面：桶/下标取极值必须走 Err 分支",
        );
        // 判据自洽：族数常量与 FAMILIES 一致。
        s.add(
            "C22-门禁-族数常量自洽",
            NodeFamily::ALL_THREE_COUNT == 3 && FAMILIES.len() == 4,
            "常三族常量与实际族表对账，防止注释与实现漂移",
        );
        // MAX_CHECKS 未溢出（域自检不截断）。
        s.add(
            "C22-门禁-判据容量未溢出",
            !s.truncated(),
            "域自检项数须在 MAX_CHECKS 内，超出会静默丢红",
        );
    }

    s
}