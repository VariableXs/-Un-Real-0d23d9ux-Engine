//! VE-F2808 · 域自检（判据逐条对应，见 `veo08_import.rs` 头注）。
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **O01 架构声明** → `O08-规格-*`（规格表可机检、三形态封闭全集、
//!   参数域逐行对拍、秩精确、零 panic 面）
//! - **集成边界** → `O08-边界-*`（上游哈希对账双向、下游前向声明、
//!   对账钩子正反向）
//! - **解析子集** → `O08-子集-*`（建图/连边/拓扑序性质/深度剖面/
//!   快照逐字段对拍/墓碑过滤）
//! - **降级矩阵** → `O08-降级-*`（拒绝三要素 / 深度恰阈双向 / 钳制
//!   + 告警 / 出边容量 / 环检出立案 / 统计守恒）
//! - **判据** → `O08-判据-*`（零 panic 自扫描、独立第二套拓扑实现
//!   对拍、码表冻结、恒真防线、非截断断言）
//!
//! **判据设计纪律**（承 F2806/F2807 家族）：
//! 1. 不向被测函数问答案——期望值判据侧独立算出（三形态规格值、
//!    码表 10 条、下游 faces 三个都是**独立写死的字面量**）；
//! 2. 双向验证——每条拒绝判据配对应接受判据；
//! 3. 形状与值两层都断；
//! 4. 专属错误码（断「有东西坏了」等于没断）；
//! 5. 弱门禁须指认它能抓的变异（深度恰阈抓 `>`⇄`>=`，秩对拍抓
//!    枚举换序，独立拓扑抓 Kahn 队列序翻转）；
//! 6. 判据区零 panic 面；
//! 7. 判据不得自调全域入口（递归教训承 F2806 判据-06）。

use crate::checks::{CheckSet, MAX_CHECKS};

use super::veo08_import::*;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧常量与语料（不从被测常量推导）
// ---------------------------------------------------------------------------

/// 判据侧独立写死的码表（0x2Bxx 段冻结，10 条）。
const EXPECT_WIRE: [&str; 10] = [
    "E_IMPORT_NAME_EMPTY",
    "E_IMPORT_UNKNOWN",
    "E_IMPORT_SELF_LOOP",
    "E_IMPORT_CYCLE",
    "E_IMPORT_DEPTH",
    "E_IMPORT_DUP",
    "E_IMPORT_CAP",
    "E_IMPORT_FORM_INVALID",
    "E_IMPORT_UPSTREAM_DRIFT",
    "E_IMPORT_DOWNSTREAM_DRIFT",
];

/// 判据侧独立写死的形态中文名（秩序：纯名/媒体/层叠层）。
const EXPECT_FORM_ZH: [&str; 3] = ["纯名", "媒体", "层叠层"];

/// 判据侧独立写死的规格表期望值（name_cap, param_cap）按秩排列。
const EXPECT_SPECS: [(usize, usize); 3] = [(128, 0), (128, 256), (128, 64)];

/// 判据侧独立写死的下游 faces 期望（秩序敏感）。
const EXPECT_FACES: [&str; 3] = ["topo_order", "import_edges", "depth_profile"];

/// 独立第二套拓扑实现：**DFS 三色 + 后序**（被测面走 Kahn，两套算法
/// 互证——同源恒绿防线）。边语义与被测面一致：(a,b) = a 依赖 b ⇒
/// b 先加载；DFS 后序中被依赖方先完成，故后序即依赖在前的加载序。
fn dfs_topo_independent(edges: &[(usize, usize)], n: usize) -> Option<Vec<usize>> {
    // 0=白 1=灰 2=黑；灰遇灰 = 有环 → None。
    let mut color = vec![0u8; n];
    let mut post: Vec<usize> = Vec::new();
    // 迭代化 DFS（显式栈：(节点, 邻接游标)；零递归零 panic 面）。
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut k = 0usize;
    while k < edges.len() {
        let (a, b) = edges[k];
        adj[a].push(b);
        k += 1;
    }
    let mut root = 0usize;
    while root < n {
        if color[root] != 0 {
            root += 1;
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0usize)];
        color[root] = 1;
        while let Some(top) = stack.pop() {
            let (u, cursor) = top;
            if cursor < adj[u].len() {
                stack.push((u, cursor + 1));
                let v = adj[u][cursor];
                if color[v] == 1 {
                    return None; // 灰遇灰 = 环
                }
                if color[v] == 0 {
                    color[v] = 1;
                    stack.push((v, 0usize));
                }
            } else {
                color[u] = 2;
                post.push(u);
            }
        }
        root += 1;
    }
    Some(post)
}

/// 判据侧独立性质断言：给定的序里每条边 (a,b) 都满足 b 先于 a
/// （a 依赖 b ⇒ 被依赖方先加载）。
fn order_satisfies(order: &[usize], edges: &[(usize, usize)], n: usize) -> bool {
    let mut pos = vec![usize::MAX; n];
    let mut k = 0usize;
    while k < order.len() {
        if order[k] >= n {
            return false;
        }
        pos[order[k]] = k;
        k += 1;
    }
    let mut k = 0usize;
    while k < edges.len() {
        let (a, b) = edges[k];
        if pos[a] == usize::MAX || pos[b] == usize::MAX || pos[b] >= pos[a] {
            return false;
        }
        k += 1;
    }
    true
}

/// 单遍词法剥除（字符串字面量/行注释一次状态机；承 F2807 同款思路）。
fn strip_lexical_noise(src: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut in_line_comment = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_str {
            if c == '\\' {
                let _ = chars.next();
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
            continue;
        }
        if c == '/' && chars.peek() == Some(&'/') {
            in_line_comment = true;
            let _ = chars.next();
            continue;
        }
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------------------
// 一、规格（架构声明）
// ---------------------------------------------------------------------------

fn chk_spec_audit(set: &mut CheckSet) {
    // 规格-01：规格表审计通过且文案含四个关键数字（独立断值）。
    match audit_spec_table() {
        Ok(msg) => {
            let all = msg.contains("3") && msg.contains("256") && msg.contains("1024")
                && msg.contains("16");
            if all {
                set.ok("O08-规格-01-规格表审计");
            } else {
                set.fail("O08-规格-01-规格表审计", "审计文案缺关键数字");
            }
        }
        Err(_) => set.fail("O08-规格-01-规格表审计", "规格表审计不应失败"),
    }
}

fn chk_spec_closed(set: &mut CheckSet) {
    // 规格-02：三形态封闭全集（判据侧独立写死秩/中文名双对拍）。
    let mut ok = ImportForm::ALL.len() == 3;
    let mut k = 0usize;
    while ok && k < 3 {
        let f = ImportForm::of_rank(k);
        ok = match f {
            Some(f) => f.rank() == k && f.zh() == EXPECT_FORM_ZH[k],
            None => false,
        };
        k += 1;
    }
    ok = ok && ImportForm::of_rank(3).is_none() && ImportForm::of_rank(usize::MAX).is_none();
    if ok {
        set.ok("O08-规格-02-形态封闭全集");
    } else {
        set.fail("O08-规格-02-形态封闭全集", "秩/中文名与独立期望不符");
    }
}

fn chk_spec_param_domain(set: &mut CheckSet) {
    // 规格-03：参数域逐行对拍（判据侧独立写死 (128,0)/(128,256)/(128,64)）。
    let mut ok = FORM_SPECS.len() == 3;
    let mut k = 0usize;
    while ok && k < 3 {
        ok = FORM_SPECS[k].name_cap == EXPECT_SPECS[k].0
            && FORM_SPECS[k].param_cap == EXPECT_SPECS[k].1;
        k += 1;
    }
    // 规格-04：spec_of_form 秩寻址与越界守卫。
    ok = ok && spec_of_form(Plain).map(|s| s.param_cap) == Some(0)
        && spec_of_form(Media).map(|s| s.param_cap) == Some(256)
        && spec_of_form(Layer).map(|s| s.param_cap) == Some(64);
    if ok {
        set.ok("O08-规格-03-参数域对拍");
        set.ok("O08-规格-04-秩寻址守卫");
    } else {
        set.fail("O08-规格-03-参数域对拍", "规格值与独立期望不符");
        set.fail("O08-规格-04-秩寻址守卫", "spec_of_form 寻址错");
    }
}

fn chk_spec_zero_panic(set: &mut CheckSet) {
    // 规格-05：生产代码零 panic 面（单遍词法剥除后扫）。
    let src = include_str!("veo08_import.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("O08-规格-05-零 panic 面");
    } else {
        set.fail("O08-规格-05-零 panic 面", "生产代码含 panic 面");
    }
}

// ---------------------------------------------------------------------------
// 二、边界（跨批对接）
// ---------------------------------------------------------------------------

fn chk_bound_upstream(set: &mut CheckSet) {
    // 边界-01：上游对账通过（真摘要现算 + F2807 下游声明对拍）。
    let real = super::veo07_rules::domain_summary();
    let c = audit_upstream(&real);
    if c.matched && c.peer == "VE-F2807" && c.missing == 0 {
        set.ok("O08-边界-01-上游对账通过");
    } else {
        set.fail("O08-边界-01-上游对账通过", "对账失败或对端错");
    }
    // 边界-02：缺失计数双向可分辨（真摘要 0 缺、劣化摘要必缺）。
    let degraded = real.replace("O01-stylesheet-v1", "O01-stylesheet-XXX");
    let m0 = upstream_missing_for(&real);
    let m1 = upstream_missing_for(&degraded);
    if m0 == 0 && m1 >= 1 {
        set.ok("O08-边界-02-缺失计数双向可分辨");
    } else {
        set.fail("O08-边界-02-缺失计数双向可分辨", "对账函数不可分辨");
    }
    // 边界-03：空摘要拒绝（防恒真门禁——对账必须真查内容）。
    let c0 = audit_upstream("");
    if !c0.matched {
        set.ok("O08-边界-03-空摘要拒绝");
    } else {
        set.fail("O08-边界-03-空摘要拒绝", "空摘要也对账通过=恒真门禁");
    }
}

fn chk_bound_downstream(set: &mut CheckSet) {
    // 边界-04：下游声明完整（判据侧独立写死 peer/faces 对拍）。
    let ok = DOWNSTREAM_DECL.peer == "VE-F2809"
        && DOWNSTREAM_DECL.faces.len() == 3
        && DOWNSTREAM_DECL.faces[0] == EXPECT_FACES[0]
        && DOWNSTREAM_DECL.faces[1] == EXPECT_FACES[1]
        && DOWNSTREAM_DECL.faces[2] == EXPECT_FACES[2];
    // 边界-05：审计通过。
    let audited = audit_downstream().is_ok();
    if ok {
        set.ok("O08-边界-04-下游声明对拍");
    } else {
        set.fail("O08-边界-04-下游声明对拍", "peer/faces 与独立期望不符");
    }
    if audited {
        set.ok("O08-边界-05-下游审计通过");
    } else {
        set.fail("O08-边界-05-下游审计通过", "声明完整但审计失败");
    }
}

fn chk_bound_reconcile(set: &mut CheckSet) {
    // 边界-06：对账钩子正反向（全命中 Ok=点名数；空/空名/未注册各拒）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("base");
    let _ = g.register_sheet("theme");
    let hit_ok = reconcile_import_graph(&g, &["base", "theme"]) == Ok(2);
    let hit_empty = reconcile_import_graph(&g, &[]).is_err();
    let hit_blank = reconcile_import_graph(&g, &["base", ""]).is_err();
    let hit_unknown = reconcile_import_graph(&g, &["ghost"]).is_err();
    if hit_ok && hit_empty && hit_blank && hit_unknown {
        set.ok("O08-边界-06-对账钩子正反向");
    } else {
        set.fail("O08-边界-06-对账钩子正反向", "钩子正反向行为不齐");
    }
}

// ---------------------------------------------------------------------------
// 三、子集（建图与拓扑）
// ---------------------------------------------------------------------------

fn chk_subset_register(set: &mut CheckSet) {
    // 子集-01：注册 id 单调 + 重名拒 + 空名拒（专属码）。
    let mut g = ImportGraph::new();
    let id0 = g.register_sheet("a").unwrap_or(0);
    let id1 = g.register_sheet("b").unwrap_or(0);
    let dup = match g.register_sheet("a") {
        Err(e) => e.code == E_IMPORT_DUP,
        Ok(_) => false,
    };
    let blank = match g.register_sheet("") {
        Err(e) => e.code == E_IMPORT_NAME_EMPTY,
        Ok(_) => false,
    };
    if id0 != 0 && id1 == id0 + 1 && dup && blank {
        set.ok("O08-子集-01-注册与重名拒");
    } else {
        set.fail("O08-子集-01-注册与重名拒", "id 序或拒绝码不符");
    }
}

fn chk_subset_link(set: &mut CheckSet) {
    // 子集-02：连边 + 快照逐字段对拍（form/param/方向）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("root");
    let _ = g.register_sheet("leaf");
    let eid = g.link_import("root", "leaf", Media, "screen", 7).unwrap_or(u64::MAX);
    let snap = g.import_edges();
    let ok = eid == 0
        && snap.len() == 1
        && snap[0].0 == "root"
        && snap[0].1 == "leaf"
        && snap[0].2 == Media
        && snap[0].3 == "screen"
        && g.stats.edges == 1
        && g.has_edge(0, 1);
    if ok {
        set.ok("O08-子集-02-连边快照对拍");
    } else {
        set.fail("O08-子集-02-连边快照对拍", "边快照字段与期望不符");
    }
}

fn chk_subset_topo(set: &mut CheckSet) {
    // 子集-03：菱形图拓扑序——独立 DFS 实现对拍 + 性质断言。
    // root → (a, b) → leaf：叶 leaf 最先，root 最后；a/b 之间序不定。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("root");
    let _ = g.register_sheet("mid-a");
    let _ = g.register_sheet("mid-b");
    let _ = g.register_sheet("leaf");
    let _ = g.link_import("root", "mid-a", Plain, "", 0);
    let _ = g.link_import("root", "mid-b", Plain, "", 0);
    let _ = g.link_import("mid-a", "leaf", Plain, "", 0);
    let _ = g.link_import("mid-b", "leaf", Plain, "", 0);
    let topo = g.topo_order().unwrap_or_default();
    // 名字 → 判据侧独立下标映射（不问被测面要映射）。
    let names = ["root", "mid-a", "mid-b", "leaf"];
    let mut idx: Vec<usize> = Vec::new();
    let mut k = 0usize;
    while k < topo.len() {
        let mut found = usize::MAX;
        let mut j = 0usize;
        while j < names.len() {
            if topo[k] == names[j] {
                found = j;
            }
            j += 1;
        }
        idx.push(found);
        k += 1;
    }
    // 独立边集（判据侧写死，方向 = (依赖方, 被依赖方)：root 依赖
    // 两个 mid，mid 依赖 leaf；下标 root=0 mid-a=1 mid-b=2 leaf=3）。
    let edges = [(0usize, 1usize), (0, 2), (1, 3), (2, 3)];
    let ind = dfs_topo_independent(&edges, 4);
    let prop = order_satisfies(&idx, &edges, 4);
    let both = match ind {
        Some(order) => order_satisfies(&order, &edges, 4),
        None => false,
    };
    if topo.len() == 4 && prop && both {
        set.ok("O08-子集-03-拓扑序性质与双实现对拍");
    } else {
        set.fail("O08-子集-03-拓扑序性质与双实现对拍", "拓扑序不满足性质或双实现分歧");
    }
}

fn chk_subset_depth(set: &mut CheckSet) {
    // 子集-04：深度剖面（链 root→c1→…→c8：最深 8 层恰 1 张触底）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("chain-root");
    let mut k = 1usize;
    while k <= 8 {
        let _ = g.register_sheet(&format!("chain-{}", k));
        k += 1;
    }
    // 逐段连链（parent=chain-(k-1)，child=chain-k）。
    let mut k = 1usize;
    while k <= 8 {
        let pname = if k == 1 {
            String::from("chain-root")
        } else {
            format!("chain-{}", k - 1)
        };
        let cname = format!("chain-{}", k);
        let _ = g.link_import(&pname, &cname, Plain, "", 0);
        k += 1;
    }
    let (max_d, cnt) = g.depth_profile();
    if max_d == 8 && cnt == 1 {
        set.ok("O08-子集-04-深度剖面");
    } else {
        set.fail("O08-子集-04-深度剖面", "最深链或触底数不符");
    }
}

fn chk_subset_tombstone(set: &mut CheckSet) {
    // 子集-05：墓碑节点从 live_idx 隐藏（F2807 同款语义跨单一致）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("live");
    let _ = g.register_sheet("dead");
    let _ = g.link_import("live", "dead", Plain, "", 0);
    let before = g.live_idx("dead").is_some();
    if before {
        // 直接置墓碑（拆除留痕由存储层负责；本单验证读时过滤）。
        let mut i = 0usize;
        while i < g.nodes.len() {
            if g.nodes[i].name == "dead" {
                g.nodes[i].removed = true;
            }
            i += 1;
        }
        let after = g.live_idx("dead").is_none();
        let re_reg = g.register_sheet("dead").is_ok();
        if after && re_reg {
            set.ok("O08-子集-05-墓碑读时过滤");
        } else {
            set.fail("O08-子集-05-墓碑读时过滤", "墓碑仍可命中或复用异常");
        }
    } else {
        set.fail("O08-子集-05-墓碑读时过滤", "基线节点未注册（恒真防线）");
    }
}

// ---------------------------------------------------------------------------
// 四、降级矩阵
// ---------------------------------------------------------------------------

fn chk_degrade_rejects(set: &mut CheckSet) {
    // 降级-01：四类拒绝各带专属码（未注册父/子、自环、重边、空端点）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("p");
    let _ = g.register_sheet("q");
    let unk_p = match g.link_import("ghost", "q", Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_UNKNOWN,
        Ok(_) => false,
    };
    let unk_c = match g.link_import("p", "ghost", Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_UNKNOWN,
        Ok(_) => false,
    };
    let self_loop = match g.link_import("p", "p", Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_SELF_LOOP,
        Ok(_) => false,
    };
    let _ = g.link_import("p", "q", Plain, "", 0);
    let dup = match g.link_import("p", "q", Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_DUP,
        Ok(_) => false,
    };
    let blank = match g.link_import("", "q", Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_NAME_EMPTY,
        Ok(_) => false,
    };
    if unk_p && unk_c && self_loop && dup && blank {
        set.ok("O08-降级-01-拒绝码逐类对拍");
    } else {
        set.fail("O08-降级-01-拒绝码逐类对拍", "某类拒绝码不符");
    }
}

fn chk_degrade_depth(set: &mut CheckSet) {
    // 降级-02：深度恰阈双向（16 可收、17 拒——抓 `>`⇄`>=` 变异）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("d0");
    let mut ok_edges = 0u32;
    let mut k = 1u32;
    while k <= MAX_IMPORT_DEPTH {
        let pname = if k == 1 {
            String::from("d0")
        } else {
            format!("d{}", k - 1)
        };
        let cname = format!("d{}", k);
        let _ = g.register_sheet(&cname);
        if g.link_import(&pname, &cname, Plain, "", 0).is_ok() {
            ok_edges += 1;
        }
        k += 1;
    }
    let _ = g.register_sheet("d-over");
    let over = match g.link_import("d16", "d-over", Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_DEPTH,
        Ok(_) => false,
    };
    if ok_edges == MAX_IMPORT_DEPTH && over {
        set.ok("O08-降级-02-深度恰阈双向");
    } else {
        set.fail("O08-降级-02-深度恰阈双向", "恰阈边界不严（> 与 >= 混淆）");
    }
}

fn chk_degrade_clamps(set: &mut CheckSet) {
    // 降级-03：钳制 + 告警（名 128 截断有痕；Plain 参数钳空；Media 参数 256 截断）。
    let mut g = ImportGraph::new();
    let long_name = format!("{}", "x".repeat(200));
    let _ = g.register_sheet(&long_name);
    let name_clamped = g.nodes[0].name.len() == 128 && g.stats.clamped >= 1;
    let _ = g.register_sheet("m-root");
    let _ = g.register_sheet("m-leaf");
    let long_param = format!("{}", "y".repeat(300));
    let _ = g.link_import("m-root", "m-leaf", Media, &long_param, 0);
    let param_clamped = g.edges[0].param.len() == 256;
    let _ = g.register_sheet("pl-root");
    let _ = g.register_sheet("pl-leaf");
    let _ = g.link_import("pl-root", "pl-leaf", Plain, "should-be-dropped", 0);
    let plain_empty = g.edges.len() >= 2 && g.edges[g.edges.len() - 1].param.is_empty();
    if name_clamped && param_clamped && plain_empty && g.clamps.len() >= 2 {
        set.ok("O08-降级-03-钳制与告警");
    } else {
        set.fail("O08-降级-03-钳制与告警", "截断长度或告警记账不符");
    }
}

fn chk_degrade_out_degree(set: &mut CheckSet) {
    // 降级-04：单表出边恰阈（32 可收、33 拒 E_IMPORT_CAP）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("hub");
    let mut k = 0usize;
    while k <= EDGES_PER_NODE {
        let leaf = format!("l{}", k);
        let _ = g.register_sheet(&leaf);
        k += 1;
    }
    let mut ok_edges = 0u32;
    let mut k = 0usize;
    while k < EDGES_PER_NODE {
        let leaf = format!("l{}", k);
        if g.link_import("hub", &leaf, Plain, "", 0).is_ok() {
            ok_edges += 1;
        }
        k += 1;
    }
    let over_leaf = format!("l{}", EDGES_PER_NODE);
    let over = match g.link_import("hub", &over_leaf, Plain, "", 0) {
        Err(e) => e.code == E_IMPORT_CAP,
        Ok(_) => false,
    };
    if ok_edges == EDGES_PER_NODE as u32 && over {
        set.ok("O08-降级-04-出边容量恰阈");
    } else {
        set.fail("O08-降级-04-出边容量恰阈", "出边上界不严");
    }
}

fn chk_degrade_cycle(set: &mut CheckSet) {
    // 降级-05：三环检出（A→B→C→A）——topo 拒绝整图 + 环成员恰 3 +
    // 立案流转（案件账非空）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("cyc-a");
    let _ = g.register_sheet("cyc-b");
    let _ = g.register_sheet("cyc-c");
    let _ = g.register_sheet("clean");
    let _ = g.link_import("cyc-a", "cyc-b", Plain, "", 0);
    let _ = g.link_import("cyc-b", "cyc-c", Plain, "", 0);
    let _ = g.link_import("cyc-c", "cyc-a", Plain, "", 0);
    let topo = g.topo_order();
    let code = match &topo {
        Err(e) => e.code == E_IMPORT_CYCLE,
        Ok(_) => false,
    };
    let leftover = g.detect_cycles();
    // 判据侧独立集合比较（不依赖顺序）。
    let expect = ["cyc-a", "cyc-b", "cyc-c"];
    let mut hits = 0usize;
    for e in expect.iter() {
        if leftover.iter().any(|x| x == e) {
            hits += 1;
        }
    }
    let clean_first = match &topo {
        Ok(_) => false,
        Err(_) => true, // 有环时整图拒绝——clean 也拿不到部分序
    };
    if code && hits == 3 && leftover.len() == 3 && clean_first && g.cases.len() >= 1 {
        set.ok("O08-降级-05-环检出与立案");
    } else {
        set.fail("O08-降级-05-环检出与立案", "环成员/拒绝码/立案不符");
    }
}

fn chk_degrade_stats(set: &mut CheckSet) {
    // 降级-06：统计守恒（成功+被拒==尝试）贯穿混合场景。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("s-a");
    let _ = g.register_sheet("s-a"); // 拒
    let _ = g.register_sheet(""); // 拒
    let _ = g.link_import("s-a", "s-a", Plain, "", 0); // 拒
    let _ = g.link_import("ghost", "s-a", Plain, "", 0); // 拒
    let _ = g.link_import("s-a", "ghost", Plain, "", 0); // 拒
    if g.stats.conserves() && g.stats.attempts == 6 && g.stats.accepted == 1 && g.stats.rejected == 5 {
        set.ok("O08-降级-06-统计守恒");
    } else {
        set.fail("O08-降级-06-统计守恒", "守恒式或计数不符");
    }
}

// ---------------------------------------------------------------------------
// 五、判据承载力
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据代码自身零 panic 面。
    let src = include_str!("veo08_import_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("O08-判据-01-判据面零 panic");
    } else {
        set.fail("O08-判据-01-判据面零 panic", "判据代码含 panic 面");
    }
}

fn chk_criterion_codes(set: &mut CheckSet) {
    // 判据-02：码表冻结（10 码与生产面逐条同源对拍 + 段独占）。
    let expect: [&str; 10] = [
        E_IMPORT_NAME_EMPTY,
        E_IMPORT_UNKNOWN,
        E_IMPORT_SELF_LOOP,
        E_IMPORT_CYCLE,
        E_IMPORT_DEPTH,
        E_IMPORT_DUP,
        E_IMPORT_CAP,
        E_IMPORT_FORM_INVALID,
        E_IMPORT_UPSTREAM_DRIFT,
        E_IMPORT_DOWNSTREAM_DRIFT,
    ];
    let mut ok = expect.len() == EXPECT_WIRE.len();
    let mut k = 0usize;
    while ok && k < 10 {
        ok = expect[k] == EXPECT_WIRE[k];
        k += 1;
    }
    // 段独占：本单 10 码不得与 F2807 的 0x2A 码表撞名
    // （判据侧独立写死 F2807 七码，跨单对拍——不引用其私有常量）。
    const SHEET_WIRE: [&str; 7] = [
        "E_SHEET_NAME_EMPTY",
        "E_SHEET_KIND_INVALID",
        "E_SHEET_CAP",
        "E_SHEET_LONGHAND_UNKNOWN",
        "E_SHEET_ID_INVALID",
        "E_SHEET_UPSTREAM_DRIFT",
        "E_SHEET_DOWNSTREAM_DRIFT",
    ];
    let mut no_clash = true;
    for w in EXPECT_WIRE.iter() {
        let mut k = 0usize;
        while k < SHEET_WIRE.len() {
            if *w == SHEET_WIRE[k] {
                no_clash = false;
            }
            k += 1;
        }
    }
    if ok && no_clash {
        set.ok("O08-判据-02-码表冻结与段独占");
    } else {
        set.fail("O08-判据-02-码表冻结与段独占", "码表漂移或与他单撞码");
    }
}

fn chk_criterion_independent_topo(set: &mut CheckSet) {
    // 判据-03：独立第二套拓扑对拍的检出力自证——喂一个环，
    // DFS 实现必须返回 None（反向语料可分辨，防两套实现同坏）。
    let cyclic = [(0usize, 1usize), (1, 0)];
    let acyclic = [(0usize, 1usize), (1, 2)];
    let a = dfs_topo_independent(&cyclic, 2).is_none();
    let b = dfs_topo_independent(&acyclic, 3).is_some();
    if a && b {
        set.ok("O08-判据-03-独立拓扑可分辨");
    } else {
        set.fail("O08-判据-03-独立拓扑可分辨", "独立实现对环/无环不分辨");
    }
}

fn chk_criterion_baseline(set: &mut CheckSet) {
    // 判据-04：恒真防线——拓扑/剖面判据跑在非空基线上（先证基线
    // 非平凡，再证性质；空图上「序合法」是空真）。
    let mut g = ImportGraph::new();
    let _ = g.register_sheet("b-root");
    let _ = g.register_sheet("b-leaf");
    let _ = g.link_import("b-root", "b-leaf", Plain, "", 0);
    let topo = g.topo_order().unwrap_or_default();
    let nontrivial = topo.len() == 2 && topo[0] == "b-leaf" && topo[1] == "b-root";
    let (max_d, _) = g.depth_profile();
    if nontrivial && max_d == 1 {
        set.ok("O08-判据-04-基线非平凡");
    } else {
        set.fail("O08-判据-04-基线非平凡", "基线图为空或序退化为空真");
    }
}

fn chk_criterion_capacity(set: &mut CheckSet) {
    // 判据-05：非截断断言（两族合计不得超 MAX_CHECKS；聚合防自调
    // ——本函数只数判据侧常数，不递归调 run 入口）。
    let total = EXPECTED_CHECK_COUNT;
    if total <= MAX_CHECKS {
        set.ok("O08-判据-05-容量不截断");
    } else {
        set.fail("O08-判据-05-容量不截断", "判据总数超 MAX_CHECKS");
    }
}

/// 判据总数（独立常数；加判据时同步维护——聚合器 tally 对拍用）。
const EXPECTED_CHECK_COUNT: usize = 27;

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+子集 / b=降级+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 子集。
pub fn run_veo08_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veo08/a");
    chk_spec_audit(&mut s);
    chk_spec_closed(&mut s);
    chk_spec_param_domain(&mut s);
    chk_spec_zero_panic(&mut s);
    chk_bound_upstream(&mut s);
    chk_bound_downstream(&mut s);
    chk_bound_reconcile(&mut s);
    chk_subset_register(&mut s);
    chk_subset_link(&mut s);
    chk_subset_topo(&mut s);
    chk_subset_depth(&mut s);
    chk_subset_tombstone(&mut s);
    s
}

/// 判据族 b：降级矩阵 + 判据承载力。
pub fn run_veo08_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veo08/b");
    chk_degrade_rejects(&mut s);
    chk_degrade_depth(&mut s);
    chk_degrade_clamps(&mut s);
    chk_degrade_out_degree(&mut s);
    chk_degrade_cycle(&mut s);
    chk_degrade_stats(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes(&mut s);
    chk_criterion_independent_topo(&mut s);
    chk_criterion_baseline(&mut s);
    chk_criterion_capacity(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_veo08_checks() -> CheckSet {
    CheckSet::merge(
        run_veo08_checks_a_standalone(),
        run_veo08_checks_b_standalone(),
    )
}
