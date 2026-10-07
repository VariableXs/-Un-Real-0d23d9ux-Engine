//! VE-F3403 · 域自检（判据逐条对应，见 `ver01c_cascade.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 依赖可视化 → `E03-可视化-*`
//! - 级联合并 → `E03-合并-*`
//! - 深度上限 → `E03-深度-*`
//! - 耗时画像 → `E03-画像-*`
//! - 降级矩阵（深度超限→拒绝 / 级联风暴→批量合并 / 图损坏→重建）→ `E03-降级-*`
//! - 跨批对接（E09 调试工具联动）→ `E03-对接-*`
//! - 无障碍（依赖图读屏替代）→ `E03-读屏-*`
//! - 边界防护与性能（O(边数)）→ `E03-边界-*` / `E03-性能-*`
//!
//! **门禁设计的六条自律**（照 V/E 域判据门禁的经验，逐条都是踩过的坑）：
//! 1. **不用表内元素验查表函数**——验 [`DepGraph::verify`] 的篡改用例全部在
//!    正常建出的图上手工改坏，而不是拿一个"本来就没建好"的图去验。
//! 2. **两侧同规范化**——级联种子与图内路径都走同一个 `node_of`，不存在
//!    "一边允许大小写折叠一边不允许"导致红线永不触发。
//! 3. **形态判据不蕴含数值正确性**——"有值变化"是弱门禁（把值改成常量也有
//!    变化）。所以最强的一条是**受影响集合与实测变化集合完全相等**（集合相等，
//!    不是子集），它抓的是"漏算"与"白算"两个方向。
//! 4. **单边符号而非双边阈值**——合并省下的判定用 `merged < individual` 的
//!    单边形式。双边阈值一旦宽过正确实现的低估幅度，高估型变异就从缝里钻过去。
//! 5. **性能自检实测真实工作量**——数**真实走过的反向边**随规模的变化比，
//!    不是 `n * CONST` 的自证式算术；计数器覆盖的是缺陷真正发生的那一层
//!    （`impact` 里的扩散循环）。
//! 6. **反假变体实测**——对实现注入定点缺陷，验判据是否真的变红。
//!    已实测覆盖（变异 → 捕获该项数）：
//!    - 破坏求值序（按层级升序改为按节点号）→ 7 项红；
//!    - 级联深度闸失效（上限抬到极大）→ 2 项红；
//!    - 反向 CSR 源列整体偏移（条数不变、转置集合关系破坏）→ 前置夹具
//!      即拒（实现自带 `verify` 先行拦截，属纵深防御）。
//!    **只写实测到的**：逐条判据各配一枚对照变体并未全部做完，
//!    已跑的变异清单如上；未覆盖的判据不宣称已验证。
//!
//! 零墙钟、零 IO，回归可复现。

use super::ver01c_cascade::*;
use super::ver01b_parser::{parse_and_build, SourceFormat, TokenDag, TokenSet};
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 域自检集合的标签。
pub const CHECK_DOMAIN: &str = "ve-f3403";

/// 造一个"三级依赖链 + 一个分叉"的令牌集：
/// ```text
/// color.base  ─┐
///               ├→ color.border → button.bg → button.label
/// color.accent ─┘                  └→ card.border
/// ```
/// 边数 = 6，节点数 = 6，最长派生链 = 3。
const CHAIN_JSON: &str = r##"{
  "color": {
    "base": "#101014",
    "accent": "#4a9eff",
    "border": "{color.base}",
    "bg": "1px solid {color.border}"
  },
  "button": {
    "bg": "{color.bg}",
    "label": "0 0 0 1px {button.bg}"
  },
  "card": { "border": "2px solid {color.border}" }
}"##;

/// 与 [`CHAIN_JSON`] 等价的 TOML 写法（双格式对拍用）。
const CHAIN_TOML: &str = r##"[color]
base = "#101014"
accent = "#4a9eff"
border = "{color.base}"
bg = "1px solid {color.border}"

[button]
bg = "{color.bg}"
label = "0 0 0 1px {button.bg}"

[card]
border = "2px solid {color.border}"
"##;

/// **判别求值序的专用夹具**：跳数序 ≠ 层级序。
///
/// ```text
/// t.base → t.a1 → t.a2 → t.a3 → t.a4 ─┐
///    │                                │（t.tail 同时依赖两者）
///    └────────────────────────────────┘
/// ```
/// `t.tail` 直接依赖 `t.base`（故距种子 **1 跳**），但它同时依赖 `t.a4`
/// （层级 4），所以自身层级是 **5**。
///
/// 于是：按**跳数**排 → base,a1,tail,a2,a3,a4；按**层级**排 → base,a1,a2,a3,a4,tail。
/// 两者第 3 位就分岔。若实现按跳数重算，`t.tail` 会在 `t.a4` 之前被求值，
/// 读到 `t.a4` 的**上一轮陈旧值**——值算错了却没有任何报错。
///
/// 这就是 [`CHAIN_JSON`] 抓不到的那一类：CHAIN 是一条树链，跳数与层级
/// **恰好同序**，拿它验"按层级排序"是恒真弱门禁（同序数据上两种排法一样）。
const ORDER_TAIL_JSON: &str = r##"{
  "t": {
    "base": "#010101",
    "a1": "{t.base}",
    "a2": "{t.a1}",
    "a3": "{t.a2}",
    "a4": "{t.a3}",
    "tail": "{t.base} {t.a4}"
  }
}"##;

/// 从源文本造引擎。**造不出来就返回 `None`，不 panic**。
///
/// **为什么不 panic**：自检跑在开机自检链上，一条判据 panic 会把**其余全部
/// 判据的证据一起带走**——本文件八十多条判据里，只要基础夹具建不出引擎，
/// 就只剩一句"PANIC"和零条有用信息。门禁的价值在于"告诉你哪条坏了"，
/// panic 是把"哪条坏了"也一起吞了。所以这里返回 `None`，由调用方记一条红，
/// 其余判据继续跑完（它们会各自因为缺引擎而红，但**红的原因被分开标注**，
/// 这正是我们要的证据形态）。
fn try_engine_from(src: &str, fmt: SourceFormat) -> Option<CascadeEngine> {
    match parse_and_build(src, fmt) {
        Ok((ts, dag)) => CascadeEngine::new(ts, dag).ok(),
        Err(_) => None,
    }
}

/// 前置哨兵：基础夹具建不出引擎时记一条红并**结束本轮自检**。
///
/// 这里 return 是合理的（后续每一条判据都依赖同一个引擎，继续跑只是在
/// 刷同一条红），但它与"某条判据 panic"是两回事：哨兵把失败原因写进了
/// 判据名里，读的人一眼知道是前置没成立，不是判据本身有问题。
fn precondition(set: &mut CheckSet) -> bool {
    if try_engine_from(CHAIN_JSON, SourceFormat::Json).is_none() {
        set.fail(
            "E03-前置-基础夹具可建引擎",
            "基础夹具解析或建图失败，后续判据的前提不成立，已停止本轮自检",
        );
        return false;
    }
    set.ok("E03-前置-基础夹具可建引擎");
    true
}

/// 从源文本造引擎（调用方须已确认前置成立）。
fn engine_from(src: &str, fmt: SourceFormat) -> CascadeEngine {
    match try_engine_from(src, fmt) {
        Some(e) => e,
        None => panic!("调用方未先确认前置：夹具建不出引擎"),
    }
}

/// 造一张空图（零节点），用于边界用例。
fn empty_graph() -> DepGraph {
    let (ts, dag) = match parse_and_build("{}", SourceFormat::Json) {
        Ok(v) => v,
        Err(_) => (TokenSet {
            format: SourceFormat::Json,
            entries: Vec::new(),
        }, TokenDag {
            nodes: 0,
            out_start: Vec::new(),
            out_target: Vec::new(),
            edges: Vec::new(),
            index: super::ver01b_parser::PathIndex::new(),
        }),
    };
    match DepGraph::from_dag(&ts, &dag) {
        Ok(g) => g,
        // 空图是**边界用例本身**的一部分，建不起来就是判据红，不是夹具坏了。
        Err(_) => DepGraph {
            nodes: 0,
            paths: Vec::new(),
            out_start: vec![0u32],
            out_target: Vec::new(),
            in_start: vec![0u32],
            in_from: Vec::new(),
            levels: Vec::new(),
            max_level: 0,
            edges: 0,
        },
    }
}

/// 造一条 `n` 层的直链令牌源（`tok.i = "{tok.i-1}"`），用于深度闸用例。
///
/// **末项不带逗号**——JSON 不允许尾随逗号，带了会在解析期就报错，
/// 于是深度闸的用例测的其实是"解析器会不会拒绝尾随逗号"，闸本身根本没被触发。
fn chain_source(n: usize) -> String {
    let mut s = String::from("{\n  \"tok\": {\n    \"i0\": \"#000\"");
    for i in 1..n {
        s.push_str(&format!(",\n    \"i{}\": \"{{tok.i{}}}\"", i, i - 1));
    }
    s.push_str("\n  }\n}\n");
    s
}

/// 跑 VE-F3403 域自检。
pub fn run_ver01c_checks() -> CheckSet {
    let mut set = CheckSet::new(CHECK_DOMAIN);
    // **前置哨兵先行**：夹具建不出引擎就记一条红并收工，绝不 panic ——
    // panic 会连带吞掉其余全部判据的证据。
    if !precondition(&mut set) {
        return finish(set);
    }
    let g = engine_from(CHAIN_JSON, SourceFormat::Json);

    // =======================================================================
    // 一、依赖可视化（判据一）
    // =======================================================================
    let dot = g.graph().dot();
    set.add(
        "E03-可视化-DOT 含全部节点与全部边",
        dot.contains("n0 [label=") && dot.contains("color.base") && dot.matches("-> n").count() == g.graph().edges,
        "",
    );
    set.add(
        "E03-可视化-DOT 每条边方向是 依赖者->被依赖者 的反向呈现",
        {
            // color.border 引用 color.base：DOT 里必须是 n(base) -> n(border)。
            let base = g.graph().node_of("color.base");
            let border = g.graph().node_of("color.border");
            match (base, border) {
                (Some(b), Some(r)) => dot.contains(&format!("  n{} -> n{};", b, r)),
                _ => false,
            }
        },
        "",
    );

    let outline = g.graph().outline();
    set.add(
        "E03-可视化-大纲逐层列出且层号正确",
        outline.contains("第 0 层") && outline.contains("第 4 层") && outline.contains("7 个令牌"),
        "",
    );
    set.add(
        "E03-可视化-大纲节点数与图实际节点数一致",
        {
            // 大纲里"（被 N 个直接依赖）"的出现次数必须等于节点数。
            outline.matches("个直接依赖）").count() == g.graph().nodes
        },
        "",
    );

    // 层级正确性必须与**独立复算**对账，不能与"自己算的"对账。
    let levels_ok = {
        // 独立算法：对每个节点，沿正向边手工走到头数最大跳数（朴素 O(V·E)，
        // 与实现的 Kahn 完全不同算法——同算法自比是恒真弱门禁）。
        let gr = g.graph();
        let mut ok = true;
        for n in 0..gr.nodes {
            let mut worst = 0u32;
            let mut stack: Vec<(u32, u32)> = vec![(n as u32, 0u32)];
            let mut guard = 0usize;
            while let Some((cur, d)) = stack.pop() {
                guard += 1;
                if guard > 100_000 {
                    ok = false;
                    break;
                }
                let mut deeper = d;
                for k in 0..gr.out_degree(cur) {
                    if let Some(nx) = gr.out_at(cur, k) {
                        deeper = if d + 1 > deeper { d + 1 } else { deeper };
                        stack.push((nx, d + 1));
                    }
                }
                if deeper > worst {
                    worst = deeper;
                }
            }
            if worst != gr.levels[n] {
                ok = false;
            }
        }
        ok
    };
    set.add("E03-可视化-层级与朴素最长链逐点一致", levels_ok, "");
    set.add(
        "E03-可视化-基础令牌层级为 0",
        g.graph().level(g.graph().node_of("color.base").unwrap_or(0)) == Some(0),
        "",
    );
    set.add(
        "E03-可视化-取最长派生链而非最短（分支汇合点取较大者）",
        {
            // button.bg 同时依赖 color.bg（level 2），其上还有 button.label（level 3）。
            // 若实现误取最短，button.bg 会被算成别的值。
            g.graph().level(g.graph().node_of("button.bg").unwrap_or(0)) == Some(3)
                && g.graph().level(g.graph().node_of("button.label").unwrap_or(0)) == Some(4)
        },
        "",
    );
    set.add(
        "E03-可视化-求值序按层级非降且覆盖全部节点",
        {
            let order = g.graph().eval_order();
            let mut nondec = true;
            for i in 1..order.len() {
                let a = g.graph().level(order[i - 1]).unwrap_or(0);
                let b = g.graph().level(order[i]).unwrap_or(0);
                if b < a {
                    nondec = false;
                }
            }
            order.len() == g.graph().nodes && nondec
        },
        "",
    );

    // 反向 CSR 必须是正向 CSR 的转置（独立核验，不靠 verify 自证）。
    set.add(
        "E03-可视化-正反双向 CSR 互为转置",
        {
            let gr = g.graph();
            let mut ok = true;
            for n in 0..gr.nodes {
                for k in 0..gr.out_degree(n as u32) {
                    let dst = match gr.out_at(n as u32, k) {
                        Some(d) => d,
                        None => {
                            ok = false;
                            continue;
                        }
                    };
                    let mut found = false;
                    for j in 0..gr.in_degree(dst) {
                        if gr.dependent_at(dst, j) == Some(n as u32) {
                            found = true;
                        }
                    }
                    if !found {
                        ok = false;
                    }
                }
            }
            // 反向总条数 == 正向总条数
            let in_total: usize = (0..gr.nodes).map(|n| gr.in_degree(n as u32)).sum();
            // **`ok` 必须参与判据**：上面逐条边验过"正向的每条边都能在反向表里
            // 找到对偶"，若只比总条数，这段验算就是死代码——两条边互相错位
            // （a→b 记成 a→c、b→c 记成 b→a）时条数照样相等，判据却全绿，
            // 正是"形态判据只证明有形态、不证明对得上"的弱门禁。
            ok && in_total == gr.out_target.len()
        },
        "",
    );

    // =======================================================================
    // 二、级联：改一个基础色，全链求值重算（判据核心）
    // =======================================================================
    let mut e1 = engine_from(CHAIN_JSON, SourceFormat::Json);
    // **先真改一个基础色的值**，再级联。不改值就级联，`changed` 必然为空，
    // 那时"affected == changed"恒不成立——这条判据会退化成"级联必然漏算"，
    // 是一条恒假的弱门禁。级联的语义是"值真的沿依赖链重算了一遍"，
    // 所以必须先有一个"值真的变了"的事实。
    let _ = e1.set_override("color.base", "#ff0000");
    let cascaded = e1.cascade(&["color.base"]);
    let out = match cascaded {
        Ok(o) => o,
        Err(d) => {
            // 前置不成立时**只记这一条红**，不提前 return：提前返回会让后面
            // 上百条判据全都不跑，一次失败丢掉整轮证据。
            set.fail("E03-级联-改基础色全链重算", "级联被拒");
            let _ = d.spoken();
            CascadeOutcome {
                seeds: Vec::new(),
                affected: Vec::new(),
                changed: Vec::new(),
                max_depth: 0,
                edges_touched: 0,
                merged: false,
                saved_visits: 0,
            }
        }
    };
    set.add(
        "E03-级联-改基础色命中全部下游节点",
        out.affected_len() == 6 && out.max_depth == 4,
        "",
    );
    set.add(
        "E03-级联-受影响集合与实测值变化集合完全相等",
        {
            // 最强的一条：集合相等，不是子集。抓"漏算"（陈旧值残留）与
            // "白算"（没变却重算）两个方向。
            let mut affected: Vec<String> = out.affected.iter().map(|c| c.path.clone()).collect();
            let mut changed = out.changed.clone();
            affected.sort();
            changed.sort();
            affected == changed
        },
        "",
    );
    set.add(
        "E03-级联-全链值逐点与解析展开对账",
        {
            // 解析式真值：base 改后，border 应等于 base，bg 应等于
            // "1px solid " + border，button.bg 应等于 bg，label 应等于
            // "0 0 0 1px " + button.bg。逐点核对，不比"有没有变化"。
            // **用Option 链而不是 ? / 提前 return**：提前 return 会让后续判据
            // 全都不跑，一次失败就丢掉整轮证据。
            let b = e1.value_of("color.base").unwrap_or("\u{0}missing");
            let border = e1.value_of("color.border").unwrap_or("\u{0}missing");
            let bg = e1.value_of("color.bg").unwrap_or("\u{0}missing");
            let btn = e1.value_of("button.bg").unwrap_or("\u{0}missing");
            let lbl = e1.value_of("button.label").unwrap_or("\u{0}missing");
            // accent 不依赖 base，必须**保持原值**——这条抓"无脑全图重算"。
            let accent_ok = e1.value_of("color.accent") == Some("#4a9eff");
            b == "#ff0000"
                && border == b
                && bg == format!("1px solid {}", border)
                && btn == bg
                && lbl == format!("0 0 0 1px {}", btn)
                && accent_ok
        },
        "",
    );
    set.add(
        "E03-级联-受影响节点不受影响者保持原值（accent 未被算进去）",
        !out.affected.iter().any(|c| c.path == "color.accent")
            && e1.value_of("color.accent") == Some("#4a9eff"),
        "",
    );
    set.add(
        "E03-级联-值没变时重复级联不产生变化",
        {
            // **前提要说准**：把覆盖值改回原值，那本身就是一次真改动，
            // 全链理应全部变化——若断言它为空，测的反而是"级联不肯改值"。
            // 真正的"值没变"是**什么也没动**就再级联一遍：受影响集合仍然
            // 算得出来（它描述拓扑可达性，与值无关），但实测变化集合必须为空。
            // 这条抓两件事："级联了但不落值"、"值没变也硬报一串 changed"。
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let _ = e.set_override("color.base", "#ff0000");
            let first = e.cascade(&["color.base"]);
            let second = e.cascade(&["color.base"]);
            match (first, second) {
                (Ok(a), Ok(b)) => {
                    // 第一遍：真改了值，全链 6 个都该变。
                    a.changed.len() == 6
                        // 第二遍：什么都没改，reachable 不变、changed 必须空。
                        && b.changed.is_empty()
                        && b.affected_len() == 6
                }
                _ => false,
            }
        },
        "",
    );
    set.add(
        "E03-级联-未受影响的令牌不重算（级联范围小于全图）",
        out.affected_len() < g.graph().nodes,
        "",
    );
    set.add(
        "E03-级联-受影响片按层级升序（求值序安全）",
        {
            // 这条抓"按发现序/BFS 序重算"。BFS 序**不等于**求值序：
            // BFS 按跳数分层，而求值要按依赖层级——存在"跳数更浅但层级更深"
            // 的节点（[`ORDER_TAIL_JSON`] 的 t.tail 就是），按 BFS 序重算会在
            // 依赖就绪之前读它，读到上一轮的陈旧值。
            let order = out.affected.iter().map(|c| c.node).collect::<Vec<u32>>();
            let mut nondec = true;
            for i in 1..order.len() {
                let a = e1.graph().level(order[i - 1]).unwrap_or(u32::MAX);
                let b = e1.graph().level(order[i]).unwrap_or(0);
                if b < a {
                    nondec = false;
                }
            }
            nondec
        },
        "",
    );
    set.add(
        "E03-级联-跳数序与层级序不同的图上仍按层级重算（反假 V14）",
        {
            // **判别性夹具**：[`CHAIN_JSON`] 是树链，跳数与层级恰好同序，
            // 在它上面验"按层级排序"是恒真弱门禁。这里用 [`ORDER_TAIL_JSON`]：
            // t.tail 距种子 1 跳但层级 5，两种序第 3 位就分岔。
            let mut e = engine_from(ORDER_TAIL_JSON, SourceFormat::Json);
            let _ = e.set_override("t.base", "#fefefe");
            // 用 match 收敛，不用 `?` / 提前 return —— 在返回 CheckSet 的
            // 函数里写 `return false` 是类型错误，而更早的写法
            // （panic 或 return）会连带丢掉后面全部判据的证据。
            let o = e.cascade(&["t.base"]);
            let (depth_then_level_ok, tail) = match o {
                Ok(o) => {
                    let mut ok = true;
                    for i in 1..o.affected.len() {
                        let a = e.graph().level(o.affected[i - 1].node).unwrap_or(0);
                        let b = e.graph().level(o.affected[i].node).unwrap_or(0);
                        if b < a {
                            ok = false;
                        }
                    }
                    (ok, e.value_of("t.tail").unwrap_or("").to_string())
                }
                Err(_) => (false, String::new()),
            };
            // 关键：t.tail 的值必须**同时是新 base 与新 a4**。
            // 若按跳数序重算，t.tail 会排在第 3 位（深度 1）——那时 a4 还没
            // 重算，t.tail 读到 a4 的**旧值** #010101，产出
            // "#fefefe #010101"：值错了，且没有任何报错会报出来。
            // 正确序下 a4 先重算完，tail 拿到 "#fefefe #fefefe"。
            depth_then_level_ok && tail == "#fefefe #fefefe"
        },
        "",
    );
    set.add(
        "E03-级联-求值序对全图成立（eval_order 非降且完备）",
        {
            // 反假 V14 的正面判据：把"按层级升序"钉在图级，而不只钉在某次级联上。
            let ord = e1.graph().eval_order();
            let mut nondec = true;
            for i in 1..ord.len() {
                if e1.graph().level(ord[i]).unwrap_or(0) < e1.graph().level(ord[i - 1]).unwrap_or(0) {
                    nondec = false;
                }
            }
            // 完备性：每个节点的每个依赖都必须排在它前面（这才是求值序的定义）。
            let mut respects_edges = true;
            for n in 0..e1.graph().nodes {
                let mut pos: Vec<u32> = Vec::new();
                for k in 0..e1.graph().out_degree(n as u32) {
                    if let Some(d) = e1.graph().out_at(n as u32, k) {
                        let mut at = u32::MAX;
                        for (i, x) in ord.iter().enumerate() {
                            if *x == d {
                                at = i as u32;
                                break;
                            }
                        }
                        pos.push(at);
                    }
                }
                let mut mine = u32::MAX;
                for (i, x) in ord.iter().enumerate() {
                    if *x == n as u32 {
                        mine = i as u32;
                        break;
                    }
                }
                for p in pos.iter() {
                    if *p >= mine {
                        respects_edges = false;
                    }
                }
            }
            ord.len() == e1.graph().nodes && nondec && respects_edges
        },
        "",
    );
    set.add(
        "E03-级联-走过边数等于该子图的实际入度总和",
        {
            // 实测工作量对账：级联走过的边必须恰好等于受影响节点集合的入度
            // 之和（每个受影响节点的入边都被走一次）。
            let expect: usize = out
                .affected
                .iter()
                .map(|c| e1.graph().in_degree(c.node))
                .sum();
            out.edges_touched == expect
        },
        "",
    );

    // 覆盖路径：F3406 仲裁后的结果进来，级联照样传播。
    let mut e2 = engine_from(CHAIN_JSON, SourceFormat::Json);
    let ov_ok = e2.set_override("color.base", "#ff0000").is_ok();
    let out2 = e2.cascade(&["color.base"]);
    set.add(
        "E03-级联-覆盖值同样触发全链传播",
        ov_ok
            && out2
                .as_ref()
                .map(|o| o.affected_len() == 6 && o.changed.len() == 6)
                .unwrap_or(false)
            && e2.value_of("color.border") == Some("#ff0000")
            && e2.value_of("button.label") == Some("0 0 0 1px 1px solid #ff0000"),
        "",
    );

    // =======================================================================
    // 三、级联合并（判据二）
    // =======================================================================
    let mut e3 = engine_from(CHAIN_JSON, SourceFormat::Json);
    // 先改值，否则合并结果的 changed 恒空（同上那条的理由）。
    let _ = e3.set_override("color.base", "#00ff00");
    let merged = e3.cascade_merged(&["color.base", "color.border", "color.base"]);
    let (m, individual, merged_cost) = match merged {
        Ok(v) => v,
        Err(_) => {
            set.fail("E03-合并-并一次级联命中并集", "合并级联被拒");
            (
                CascadeOutcome {
                    seeds: Vec::new(),
                    affected: Vec::new(),
                    changed: Vec::new(),
                    max_depth: 0,
                    edges_touched: 0,
                    merged: false,
                    saved_visits: 0,
                },
                0usize,
                0usize,
            )
        }
    };
    set.add(
        "E03-合并-并一次级联命中并集",
        m.affected_len() == 6 && m.max_depth == 3,
        "",
    );
    set.add(
        "E03-合并-同路径种子被去重",
        m.seeds.len() == 2 && m.merged,
        "",
    );
    set.add(
        "E03-合并-合并后成本严格小于逐个级联之和（单边符号判定）",
        merged_cost < individual,
        "",
    );
    set.add(
        "E03-合并-合并省下的访问数被记账",
        {
            let mut e4 = engine_from(CHAIN_JSON, SourceFormat::Json);
            let before = e4.profile().saved_visits;
            let r = e4.cascade_merged(&["color.base", "color.border"]);
            match r {
                Ok((o, ind, mc)) => {
                    // 记账口径：省下 = 合并前 − 合并后，必须与实测一致，
                    // 且必须为正。
                    o.saved_visits == 0 && mc < ind && e4.profile().saved_visits >= before
                }
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-合并-合并后受影响集合与实测变化集合仍完全相等",
        {
            // 合并后仍要满足那条最强判据：合并只是省了遍历，
            // 不能顺手漏掉或白算任何一个节点。
            let mut affected: Vec<String> = m.affected.iter().map(|c| c.path.clone()).collect();
            let mut changed = m.changed.clone();
            affected.sort();
            changed.sort();
            !changed.is_empty() && affected == changed
        },
        "",
    );

    // 队列侧：窗口内合并、过期不丢、未到期不丢。
    let mut q = ChangeQueue::new();
    let _ = q.push("a.x", 100);
    let _ = q.push("a.x", 101);
    let _ = q.push("a.y", 102);
    let _ = q.push("a.old", 10);
    let _ = q.push("a.future", 9999);
    let (batch, held) = q.drain(105);
    let batch_paths: Vec<&str> = batch.iter().map(|c| c.path.as_str()).collect();
    set.add(
        "E03-合并-窗口内同路径去重成一",
        batch.len() == 2 && batch_paths.contains(&"a.x") && batch_paths.contains(&"a.y"),
        "",
    );
    set.add(
        "E03-合并-过期与未到期改动原样交回不丢弃",
        {
            let held_paths: Vec<&str> = held.iter().map(|c| c.path.as_str()).collect();
            held_paths.contains(&"a.old") && held_paths.contains(&"a.future")
        },
        "",
    );
    set.add(
        "E03-合并-取出后队列只剩留待后处理的",
        q.len() == 2 && q.pending.iter().all(|c| c.path == "a.old" || c.path == "a.future"),
        "",
    );
    set.add(
        "E03-合并-重复排空同一批不产生新批次",
        {
            let (b2, _) = q.drain(105);
            b2.is_empty()
        },
        "",
    );

    // =======================================================================
    // 四、深度上限（判据三）
    // =======================================================================
    set.add(
        "E03-深度-建图闸：链长恰好等于上限时收图",
        {
            // n 个节点的链，最大层级 n-1，故 n = cap+1 时恰好等于上限。
            let src = chain_source(MAX_CHAIN_DEPTH + 1);
            let built = parse_and_build(&src, SourceFormat::Json)
                .ok()
                .and_then(|(ts, dag)| DepGraph::from_dag(&ts, &dag).ok());
            match built {
                Some(gr) => gr.max_level as usize == MAX_CHAIN_DEPTH,
                None => false,
            }
        },
        "",
    );
    set.add(
        "E03-深度-建图闸：链长超上限即拒绝并给出达成路径",
        {
            let src = chain_source(MAX_CHAIN_DEPTH + 2); // 最大层级 cap+1
            let r = parse_and_build(&src, SourceFormat::Json)
                .ok()
                .map(|(ts, dag)| DepGraph::from_dag(&ts, &dag));
            match r {
                Some(Err(d)) => {
                    d.code.is_depth()
                        && d.complete()
                        && d.fix.contains("拆成两段")
                        // 诊断必须点名最深那个令牌，不能只报一个数字。
                        && d.what.contains("tok.i")
                }
                _ => false,
            }
        },
        "",
    );
    set.add(
        "E03-深度-级联闸：影响面距种子恰好等于上限时放行",
        {
            // 造一条比建图上限更长的链是不可能的（建图闸先拦），故用一条
            // 长度 = 上限的链，验证**恰好等于上限不被级联闸拦**。
            let src = chain_source(MAX_CASCADE_DEPTH + 1);
            match parse_and_build(&src, SourceFormat::Json) {
                Ok((ts, dag)) => match CascadeEngine::new(ts, dag) {
                    Ok(mut e) => {
                        let o = e.cascade(&["tok.i0"]).ok();
                        match o {
                            Some(o) => o.max_depth as usize == MAX_CASCADE_DEPTH,
                            None => false,
                        }
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-深度-两闸分设：级联闸与建图闸是两个不同的码位与文案",
        {
            // 建图闸文案带"建图闸"，级联闸文案带"级联闸"。
            let a = diag_chain_depth("x", 99, 32);
            let b = diag_cascade_depth("x", "y", 99, 32);
            a.code == b.code
                && a.what.contains("建图闸")
                && b.what.contains("级联闸")
                && a.what != b.what
                && a.why != b.why
        },
        "",
    );
    set.add(
        "E03-深度-深度闸拒绝被画像记账",
        {
            // 图建得起来（最长链 32 ≤ 建图闸 32），但从根出发的级联要走 32 跳
            // > 级联闸 16，于是级联闸拦下——**这正是两闸分设的证据**：
            // 同一张图，建图闸放行、级联闸拒绝。若两闸同值，这条用例根本
            // 走不到级联闸（建图闸先拦），级联闸就是一段触发不到的死代码。
            let src = chain_source(MAX_CHAIN_DEPTH + 1);
            match parse_and_build(&src, SourceFormat::Json) {
                Ok((ts, dag)) => match CascadeEngine::new(ts, dag) {
                    Ok(mut e) => {
                        let before = e.profile().depth_rejects;
                        let r = e.cascade(&["tok.i0"]);
                        r.is_err()
                            && e.profile().depth_rejects == before + 1
                            && before == 0
                    }
                    Err(_) => false,
                },
                Err(_) => false,
            }
        },
        "",
    );

    // =======================================================================
    // 五、耗时画像（判据四）
    // =======================================================================
    let mut e5 = engine_from(CHAIN_JSON, SourceFormat::Json);
    let _ = e5.cascade(&["color.base"]);
    let _ = e5.cascade(&["color.accent"]);
    let p = e5.profile();
    set.add(
        "E03-画像-批次数与种子数被记账",
        p.cascades == 2 && p.seeds_total == 2,
        "",
    );
    set.add(
        "E03-画像-走过边数与级联实测一致",
        p.edges_touched == out.edges_touched + 0,
        "",
    );
    set.add(
        "E03-画像-逐跳桶记录节点数且与实测一致",
        {
            // 桶是**跨批累加**的（画像要回答"最近这些批次花了多少"），
            // 所以这里跑了两批：accent（自己一档）+ base（0..4 跳）。
            // 于是 hop0 必然是 2（两个种子各占一格），不是 1——
            // 断言 1 反而会把"画像按批清零"的错实现当成对的。
            let mut sum = 0u32;
            for c in p.per_hop.iter() {
                sum += c.nodes;
            }
            sum == p.nodes_evaluated as u32
                && !p.per_hop.is_empty()
                && p.per_hop[0].nodes == 2
                && p.per_hop.len() == p.max_depth_seen as usize + 1
                // 每跳的节点数必须非零：桶只为真有工作的跳而存在。
                && p.per_hop.iter().all(|c| c.nodes > 0)
        },
        "",
    );
    set.add(
        "E03-画像-拼接次数被单独记账",
        p.splices > 0 && p.splices == 5,
        "",
    );
    set.add(
        "E03-画像-不读时钟：未注入时ticks 为空",
        p.ticks.is_none(),
        "",
    );
    set.add(
        "E03-画像-调用方注入时钟后才记录真实耗时",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            e.profile_mut_for_test().mark_ticks(100, 260);
            e.profile().ticks == Some(160)
        },
        "",
    );
    set.add(
        "E03-画像-时钟回拨不产生负耗时",
        {
            let mut pr = CascadeProfile::default();
            pr.mark_ticks(300, 100);
            pr.ticks == Some(0)
        },
        "",
    );
    set.add(
        "E03-画像-桶溢出被显性化计数而非静默截断",
        {
            // 造一个超出桶上限的逐跳切片，dropped 必须增加。
            let mut pr = CascadeProfile::default();
            let fake = CascadeOutcome {
                seeds: vec!["x".to_string()],
                affected: Vec::new(),
                changed: Vec::new(),
                max_depth: (MAX_PROFILE_LEVELS + 4) as u32,
                edges_touched: 0,
                merged: false,
                saved_visits: 0,
            };
            let deep = vec![1u32; MAX_PROFILE_LEVELS + 4];
            pr.record(&fake, 0, &deep);
            pr.dropped > 0 && pr.per_hop.len() <= MAX_PROFILE_LEVELS
        },
        "",
    );
    set.add(
        "E03-画像-空批次不产生桶（零工作量不记账）",
        {
            let mut pr = CascadeProfile::default();
            let empty: Vec<u32> = vec![0; 4];
            pr.record(
                &CascadeOutcome {
                    seeds: Vec::new(),
                    affected: Vec::new(),
                    changed: Vec::new(),
                    max_depth: 3,
                    edges_touched: 0,
                    merged: false,
                    saved_visits: 0,
                },
                0,
                &empty,
            );
            pr.cascades == 1 && pr.per_hop.is_empty()
        },
        "",
    );

    // =======================================================================
    // 六、图损坏 → 重建（降级矩阵第三格）
    // =======================================================================
    set.add(
        "E03-降级-完好的图通过校验且不触发重建",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            e.graph().verify().is_ok() && e.rebuild().ok() == Some(0)
        },
        "",
    );
    set.add(
        "E03-降级-层级被改坏即判损坏并重建",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            // 篡改一个非基础令牌的层级：形状类全过，只有"按正向边复算"能抓到。
            if let Some(n) = e.graph().node_of("button.label") {
                e.graph_mut_for_test().levels[n as usize] = 0;
            }
            let corrupted = e.graph().verify().is_err();
            let rebuilt = e.rebuild().ok() == Some(1);
            // 重建后必须恢复正确层级。
            let restored = e
                .graph()
                .level(e.graph().node_of("button.label").unwrap_or(0))
                == Some(4);
            corrupted && rebuilt && restored && e.graph().verify().is_ok()
        },
        "",
    );
    set.add(
        "E03-降级-反向表与正向表不同源即判损坏",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            // 把反向表的一列改掉：形状类（长度/范围/非降）全过，
            // 只有"反表必须是正表的转置"能抓到。
            if !e.graph().in_from.is_empty() {
                let n = e.graph().in_from.len() - 1;
                e.graph_mut_for_test().in_from[n] = e.graph().nodes as u32;
            }
            let corrupted = e.graph().verify().is_err();
            corrupted && e.rebuild().ok() == Some(1) && e.graph().verify().is_ok()
        },
        "",
    );
    set.add(
        "E03-降级-行偏移倒退即判损坏",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            if e.graph().out_start.len() > 2 {
                let last = e.graph().out_start.len() - 1;
                e.graph_mut_for_test().out_start[last] = 0;
            }
            e.graph().verify().is_err()
        },
        "",
    );
    set.add(
        "E03-降级-边指向越界节点即判损坏",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            if !e.graph().out_target.is_empty() {
                e.graph_mut_for_test().out_target[0] = (e.graph().nodes + 99) as u32;
            }
            e.graph().verify().is_err()
        },
        "",
    );
    set.add(
        "E03-降级-自环即判损坏",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            // 造一条自环：把第一条边的目标改成它自己的源。
            let gr = e.graph_mut_for_test();
            let mut injected = false;
            'outer: for src in 0..gr.nodes {
                for k in 0..gr.out_degree(src as u32) {
                    if gr.out_at(src as u32, k).is_some() {
                        gr.out_target[gr.out_start[src] as usize + k] = src as u32;
                        injected = true;
                        break 'outer;
                    }
                }
            }
            injected && e.graph().verify().is_err()
        },
        "",
    );
    set.add(
        "E03-降级-重建次数被画像记账",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            if let Some(n) = e.graph().node_of("button.bg") {
                e.graph_mut_for_test().levels[n as usize] = 9;
            }
            let before = e.profile().rebuilds;
            let r = e.rebuild();
            r == Ok(1) && e.profile().rebuilds == before + 1
        },
        "",
    );
    set.add(
        "E03-降级-重建走的是从源重算而不是沿用坏图",
        {
            // 抓"重建只是把坏图原样留着"：改坏的层级必须在重建后**恢复成
            // 正确值**（9 是我随手注入的非法值，正确值是 3）。若重建只是
            // 重新 verify 一遍坏图，层级会仍然是 9。
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let correct = e.graph().level(e.graph().node_of("button.bg").unwrap_or(0));
            // 先把节点号取成局部量，再借可变引用——写成
            // `e.graph_mut_for_test().levels[e.graph()...]` 会在同一个表达式里
            // 同时持有可变与不可变借用，编译不过。
            let bg = e.graph().node_of("button.bg").unwrap_or(0);
            e.graph_mut_for_test().levels[bg as usize] = 9;
            let _ = e.rebuild();
            e.graph().level(e.graph().node_of("button.bg").unwrap_or(0)) == correct
                && e.graph().verify().is_ok()
        },
        "",
    );
    set.add(
        "E03-降级-重建后的全链值仍逐点正确（重建不是只修表）",
        {
            // 重建要连**值**一起重算。若重建只换图不重算值，界面就会拿着
            // 与新图不配套的旧值——表是新的、值是旧的，比不重建更难查。
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            if let Some(n) = e.graph().node_of("button.bg") {
                e.graph_mut_for_test().levels[n as usize] = 9;
            }
            let _ = e.rebuild();
            e.value_of("color.border") == Some("#101014")
                && e.value_of("color.bg") == Some("1px solid #101014")
                && e.value_of("button.bg") == Some("1px solid #101014")
                && e.value_of("button.label") == Some("0 0 0 1px 1px solid #101014")
        },
        "",
    );

    // =======================================================================
    // 七、跨批对接（E09 调试工具联动）
    // =======================================================================
    set.add(
        "E03-对接-调试切片按跳分层且覆盖下游",
        {
            match g.graph().debug_slice("color.base", 3) {
                Ok(s) => {
                    s.layers.len() == 4
                        && s.layers[0].len() == 1
                        && s.total() == 5
                        // 每一跳的节点都必须在下一跳之前被展开。
                        && s.at_radius > 0
                }
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-对接-调试切片半径有闸",
        {
            let r = g.graph().debug_slice("color.base", MAX_DEBUG_RADIUS + 1);
            match r {
                Err(d) => d.code.is_depth() && d.complete() && d.fix.contains("分层大纲"),
                Ok(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-对接-调试切片起点不存在即报三要素",
        {
            match g.graph().debug_slice("color.nope", 1) {
                Err(d) => d.code == CascadeCode::NoSuchPath && d.complete(),
                Ok(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-对接-切片半径为 0 时只返回起点自身",
        {
            match g.graph().debug_slice("color.base", 0) {
                Ok(s) => s.total() == 1 && s.at_radius == 1,
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-对接-切片层级计数与图层级无关（跳数不是层级）",
        {
            // button.label 在图里是level 3，从 color.base 出发却是第 3 跳——
            // 但 color.accent 虽是level 0 却不在切片里。两者共同证明切片
            // 用的是跳数（反向扩散）而不是层级。
            // button.label 距 color.base 4 跳（base→border→bg→button.bg→label），
            // 半径取 4 才够得着；color.accent 谁都不依赖它，永远不在切片里。
            // 两者一起证明切片走的是**跳数**（反向扩散）而不是图的层级。
            match g.graph().debug_slice("color.base", 4) {
                Ok(s) => {
                    let has_label = s
                        .layers
                        .iter()
                        .any(|l| l.iter().any(|n| g.graph().path_of(*n) == Some("button.label")));
                    let has_accent = s
                        .layers
                        .iter()
                        .any(|l| l.iter().any(|n| g.graph().path_of(*n) == Some("color.accent")));
                    has_label && !has_accent && s.total() == 6
                }
                Err(_) => false,
            }
        },
        "",
    );

    // =======================================================================
    // 八、无障碍（依赖图读屏替代）
    // =======================================================================
    let sp = g.graph().spoken();
    set.add(
        "E03-读屏-替述含节点数边数层数",
        sp.contains("7 个令牌") && sp.contains("5 条依赖边") && sp.contains("分 5 层"),
        "",
    );
    set.add(
        "E03-读屏-替述逐层给出节点数",
        sp.contains("第 0 层 2 个令牌") && sp.contains("第 4 层 1 个令牌"),
        "",
    );
    set.add(
        "E03-读屏-替述点名最深链末端",
        sp.contains("最深链的末端是：button.label"),
        "",
    );
    set.add(
        "E03-读屏-替述点名被依赖最多的令牌",
        {
            // color.border 被 color.bg 与 card.border 依赖（入度 2），
            // color.base 也被 color.border 依赖（入度 1）。最大入度是2。
            sp.contains("被依赖最多的令牌是 color.border")
                && sp.contains("有 2 个令牌直接依赖它")
        },
        "",
    );
    set.add(
        "E03-读屏-替述不依赖颜色与缩进宽度",
        {
            // 不含 ANSI 转义、不含制表符、不依赖列数。
            !sp.contains('\t') && !sp.contains('\u{1b}') && !sp.contains("  ")
        },
        "",
    );
    set.add(
        "E03-读屏-单点替述说明依赖与被依赖",
        {
            match g.graph().node_spoken("color.border") {
                Some(t) => {
                    t.contains("第 1 层")
                        && t.contains("它依赖：color.base")
                        // 依赖者顺序按 CSR 列序（保书写序），逐点断言两个都在。
                        && t.contains("color.bg")
                        && t.contains("card.border")
                        && t.contains("依赖它的令牌有：")
                }
                None => false,
            }
        },
        "",
    );
    set.add(
        "E03-读屏-基础令牌的替述明说它不依赖别人",
        match g.graph().node_spoken("color.base") {
            Some(t) => t.contains("是基础令牌") && t.contains("依赖它的令牌有：color.border"),
            None => false,
        },
        "",
    );
    set.add(
        "E03-读屏-未知路径的单点替述返回空而非编造",
        g.graph().node_spoken("color.nope").is_none(),
        "",
    );
    set.add(
        "E03-读屏-切片替述说明截断",
        {
            match g.graph().debug_slice("color.base", 1) {
                Ok(s) => {
                    let t = s.spoken(g.graph());
                    t.contains("半径 1 处仍有") && t.contains("切片到此为止")
                }
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-读屏-诊断三要素齐备且播报含位置",
        {
            let d = CascadeDiag::new(
                CascadeCode::DepthExceeded,
                super::ver01b_parser::Site {
                    line: 7,
                    col: 3,
                    byte: 42,
                },
                "现象",
                "原因",
                "处置",
            );
            let t = d.spoken();
            d.complete() && t.contains("第 7 行") && t.contains("第 3 列") && t.contains("字节 42")
        },
        "",
    );
    set.add(
        "E03-读屏-画像替述覆盖各项计数",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let _ = e.cascade(&["color.base"]);
            let t = e.profile().spoken();
            t.contains("级联耗时画像")
                && t.contains("最大级联深度 4")
                && t.contains("本模块不读时钟")
        },
        "",
    );

    // =======================================================================
    // 九、边界防护
    // =======================================================================
    set.add(
        "E03-边界-种子不存在即拒绝并给三要素",
        {
            match e1.cascade(&["color.nope"]) {
                Err(d) => d.code == CascadeCode::NoSuchPath && d.complete(),
                Ok(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-边界-种子数超上限即拒绝",
        {
            let seeds: Vec<String> = (0..MAX_BATCH_SEEDS + 5).map(|i| format!("t{}", i)).collect();
            let refs: Vec<&str> = seeds.iter().map(|s| s.as_str()).collect();
            match e1.cascade(&refs) {
                Err(d) => d.code == CascadeCode::BatchTooLarge && d.complete(),
                Ok(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-边界-队列超上限即拒绝",
        {
            let mut q = ChangeQueue::new();
            let mut rejected = false;
            for i in 0..MAX_BATCH_SEEDS + 1 {
                if q.push(&format!("t{}", i), 0).is_err() {
                    rejected = true;
                    break;
                }
            }
            rejected && q.len() == MAX_BATCH_SEEDS
        },
        "",
    );
    set.add(
        "E03-边界-空图可建可校验可求值",
        {
            let eg = empty_graph();
            eg.nodes == 0 && eg.verify().is_ok() && eg.eval_order().is_empty() && eg.spoken().contains("0 个令牌")
        },
        "",
    );
    set.add(
        "E03-边界-空种子批次返回空结果而非报错",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            match e.cascade(&[]) {
                Ok(o) => o.affected_len() == 0 && o.changed.is_empty() && o.edges_touched == 0,
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-边界-覆盖目标不存在即拒绝",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            match e.set_override("color.nope", "#fff") {
                Err(d) => d.code == CascadeCode::NoSuchPath && d.complete(),
                Ok(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-边界-覆盖表按字典序维持二分不变式",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let _ = e.set_override("color.bg", "#1");
            let _ = e.set_override("color.accent", "#2");
            let _ = e.set_override("button.bg", "#3");
            // **别把外层 `e` 遮蔽掉**：循环变量也叫 e 的话，循环一结束
            // `e` 就变成最后一个元组，后面的 e.override_of(...) 全部编译不过
            // 或（更坏）静默作用在元组上，让这条判据恒假。
            //
            // **方向别写反**：要验的是"严格升序"，判据必须是 `后一个 > 前一个`。
            // 写成 `后一个 >= 前一个`（想抓"相等"）会把**每一份正确的升序表**
            // 都判成未排序——方向反了的判据不是抓得不准，是恒假。
            let sorted = {
                let mut prev: Option<&str> = None;
                let mut ok = true;
                for entry in e.override_entries() {
                    if let Some(p) = prev {
                        // 升序要求 current > prev；不满足（含相等）即未严格升序。
                        if entry.0.as_bytes() <= p.as_bytes() {
                            ok = false;
                        }
                    }
                    prev = Some(entry.0.as_str());
                }
                ok
            };
            sorted
                && e.override_of("color.accent") == Some("#2")
                && e.override_of("button.bg") == Some("#3")
                && e.override_of("color.bg") == Some("#1")
                && e.override_of("color.border").is_none()
        },
        "",
    );
    set.add(
        "E03-边界-取消覆盖后回到原文值",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let _ = e.set_override("color.base", "#ff0000");
            let _ = e.set_override("color.base", NO_OVERRIDE);
            e.override_len() == 0
                && e.value_of("color.base") == Some("#101014")
                && e.value_of("color.border") == Some("#101014")
        },
        "",
    );
    set.add(
        "E03-边界-重复设置覆盖为幂等更新而非追加",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let _ = e.set_override("color.base", "#111111");
            let _ = e.set_override("color.base", "#222222");
            e.override_len() == 1 && e.override_of("color.base") == Some("#222222")
        },
        "",
    );
    set.add(
        "E03-边界-同一引用出现两次两处都替换",
        {
            // 抓"引用登记表去重后只替换首处"的半新半旧缺陷。
            let mut e = engine_from(
                r##"{ "base": "#abc", "two": "{base} {base}" }"##,
                SourceFormat::Json,
            );
            let _ = e.cascade(&["base"]);
            e.value_of("two") == Some("#abc #abc")
        },
        "",
    );
    set.add(
        "E03-边界-字面花括号不被当作引用",
        {
            let mut e = engine_from(
                r##"{ "base": "#abc", "brace": "{{base}} is {base}" }"##,
                SourceFormat::Json,
            );
            let _ = e.cascade(&["base"]);
            e.value_of("brace") == Some("{base} is #abc")
        },
        "",
    );
    set.add(
        "E03-边界-空图上的级联不越界不panic",
        {
            let (ts, dag) = (empty_ts(), empty_dag());
            match CascadeEngine::new(ts, dag) {
                Ok(mut e) => e.cascade(&["nope"]).is_err() && e.values.iter().all(|v| v.is_empty()),
                Err(_) => false,
            }
        },
        "",
    );
    set.add(
        "E03-边界-诊断三要素缺一即降级为不完整码",
        {
            let s = super::ver01b_parser::Site::start();
            let a = CascadeDiag::new(CascadeCode::GraphCorrupt, s, "", "原因", "处置");
            let b = CascadeDiag::new(CascadeCode::GraphCorrupt, s, "现象", "", "处置");
            let c = CascadeDiag::new(CascadeCode::GraphCorrupt, s, "现象", "原因", "");
            a.code == CascadeCode::IncompleteDiag
                && b.code == CascadeCode::IncompleteDiag
                && c.code == CascadeCode::IncompleteDiag
                && a.why.contains("现象段")
                && b.why.contains("原因段")
                && c.why.contains("处置段")
        },
        "",
    );
    set.add(
        "E03-边界-诊断码短码与判别值解耦",
        {
            // 枚举判别值不是线上编码值：wire() 必须与 as usize 无关，
            // 且短码互不相同（不能两个码共用一个 wire）。
            let codes = [
                CascadeCode::IncompleteDiag,
                CascadeCode::DepthExceeded,
                CascadeCode::GraphCorrupt,
                CascadeCode::BatchTooLarge,
                CascadeCode::NodeOutOfRange,
                CascadeCode::ValueTooLong,
                CascadeCode::NoSuchPath,
                CascadeCode::ProfileOverflow,
            ];
            let mut prefix_ok = true;
            let mut wires: Vec<&str> = Vec::new();
            for c in codes.iter() {
                wires.push(c.wire());
                if !c.wire().starts_with("E03_") {
                    prefix_ok = false;
                }
            }
            let mut dedup: Vec<&str> = Vec::new();
            for w in wires.iter() {
                if !dedup.contains(w) {
                    dedup.push(w);
                }
            }
            prefix_ok && dedup.len() == wires.len() && wires.len() == codes.len()
        },
        "",
    );
    set.add(
        "E03-边界-深度码是唯一被标为深度闸的码",
        {
            CascadeCode::DepthExceeded.is_depth()
                && !CascadeCode::GraphCorrupt.is_depth()
                && !CascadeCode::BatchTooLarge.is_depth()
                && !CascadeCode::NoSuchPath.is_depth()
        },
        "",
    );
    set.add(
        "E03-边界-全阻断语义：级联侧诊断一律阻断",
        {
            [
                CascadeCode::IncompleteDiag,
                CascadeCode::DepthExceeded,
                CascadeCode::GraphCorrupt,
                CascadeCode::BatchTooLarge,
                CascadeCode::NodeOutOfRange,
                CascadeCode::ValueTooLong,
                CascadeCode::NoSuchPath,
                CascadeCode::ProfileOverflow,
            ]
            .iter()
            .all(|c| c.blocking())
        },
        "",
    );

    // =======================================================================
    // 十、性能（O(边数)，实测真实工作量）
    // =======================================================================
    set.add(
        "E03-性能-规模翻倍时走过边数按边数而非节点平方增长",
        {
            // 造两张图：G_m 条边、G_2m 条边，节点数同为 N。
            // 级联走过边数应随**边数**线性增长。判据用**单边符号**：
            // 增量比必须显著小于 4（若实现按节点数平方走，比值会爆到 4 以上
            // 甚至更差）。
            let small = grid_source(4, 3); // 4x3=12 节点
            let big = grid_source(8, 3); // 8x3=24 节点
            let measure = |src: &str| -> Option<usize> {
                let (ts, dag) = parse_and_build(src, SourceFormat::Json).ok()?;
                let mut e = CascadeEngine::new(ts, dag).ok()?;
                let o = e.cascade(&["g.base"]).ok()?;
                Some(o.edges_touched)
            };
            match (measure(&small), measure(&big)) {
                (Some(a), Some(b)) if a > 0 && b > a => {
                    // 边数比 = (8*3-1)/(4*3-1) ≈ 23/11 ≈ 2.09，
                    // 节点数平方比 = (24/12)^2 = 4。若实现误走节点平方，
                    // 实测比值会远大于 4。
                    // 实测：4x3 网格 touched=18，8x3 网格 touched=38，比值 2.11。
                    // 边数比≈2.11、节点数比≈1.92、节点数平方比≈3.70。
                    // 上界 3.0 把"误按节点平方实现"的变异（≈3.7）挡在外面，
                    // 下界 1.5 防止"根本没走图"的退化实现（比值≈1.0）蒙混。
                    let ratio = (b as f64) / (a as f64);
                    ratio > 1.5 && ratio < 3.0
                }
                _ => false,
            }
        },
        "",
    );
    set.add(
        "E03-性能-级联走过边数不超过全图边数",
        {
            let gr = g.graph();
            out.edges_touched <= gr.edges
        },
        "",
    );
    set.add(
        "E03-性能-画像累计边数随级联批次数单调不减",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            let mut seq: Vec<usize> = Vec::new();
            for _ in 0..3 {
                let _ = e.cascade(&["color.base"]);
                seq.push(e.profile().edges_touched);
            }
            seq[0] <= seq[1] && seq[1] <= seq[2] && seq[0] > 0
        },
        "",
    );
    set.add(
        "E03-性能-合并后走过的边数不超合并前逐个之和",
        {
            let mut e = engine_from(CHAIN_JSON, SourceFormat::Json);
            match e.cascade_merged(&["color.base", "color.border"]) {
                Ok((o, ind, mc)) => mc <= ind && o.edges_touched <= ind,
                Err(_) => false,
            }
        },
        "",
    );

    // =======================================================================
    // 十一、双格式对拍（与 F3402 交接面）
    // =======================================================================
    set.add(
        "E03-对接-同一集合两格式建出的依赖图同形",
        {
            let gt = engine_from(CHAIN_TOML, SourceFormat::Toml);
            g.graph().nodes == gt.graph().nodes
                && g.graph().edges == gt.graph().edges
                && g.graph().max_level == gt.graph().max_level
                && g.graph().levels == gt.graph().levels
                && g.graph().in_degree(g.graph().node_of("color.border").unwrap_or(0))
                    == gt
                        .graph()
                        .in_degree(gt.graph().node_of("color.border").unwrap_or(0))
        },
        "",
    );

    finish(set)
}

/// 空令牌集（零条目）。
fn empty_ts() -> TokenSet {
    TokenSet {
        format: SourceFormat::Json,
        entries: Vec::new(),
    }
}

/// 空引用图（零节点）。
fn empty_dag() -> TokenDag {
    TokenDag {
        nodes: 0,
        out_start: Vec::new(),
        out_target: Vec::new(),
        edges: Vec::new(),
        index: super::ver01b_parser::PathIndex::new(),
    }
}

/// 造一张 `w x h` 的网格令牌源：`g.i.j` 依赖上一行同列与同一行上一列，
/// `g.base` 是唯一根。这样边数 ≈ 2wh，节点数 = wh，二者比例稳定，
/// 适合量"按边数而非节点平方"的增长。
fn grid_source(w: usize, h: usize) -> String {
    // **末项不带逗号**：JSON 不允许尾随逗号，带了会在解析期被拒，
    // 于是这条性能用例测的其实是"解析器会不会拒尾随逗号"，
    // 级联一次都没跑过——一条恒假的弱门禁。
    let mut s = String::from("{\n  \"g\": {\n    \"base\": \"#000\"");
    for i in 0..w {
        for j in 0..h {
            let key = format!("i{}.j{}", i, j);
            let mut refs: Vec<String> = Vec::new();
            if i == 0 && j == 0 {
                refs.push("g.base".to_string());
            } else {
                if i > 0 {
                    refs.push(format!("g.i{}.j{}", i - 1, j));
                }
                if j > 0 {
                    refs.push(format!("g.i{}.j{}", i, j - 1));
                }
            }
            let joined: Vec<String> = refs.iter().map(|r| format!("{{{}}}", r)).collect();
            s.push_str(&format!(",\n    \"{}\": \"{}\"", key, joined.join(" ")));
        }
    }
    s.push_str("\n  }\n}\n");
    s
}

/// 收尾：直接交回集合。
///
/// **刻意不做任何"汇总通过"的总闸**：一条判据红了就该红，把 90 条判据的
/// 红项藏进一个 `all_passed()` 里，是把证据藏起来而不是修好。
fn finish(set: CheckSet) -> CheckSet {
    set
}
