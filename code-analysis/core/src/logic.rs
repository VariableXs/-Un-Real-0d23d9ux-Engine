//! 逻辑链追踪与重建（#038~#064）。
//!
//! 消费统一语义 IR（ProjectIR）与行级语句事实，产出调用图/CFG/DFG/状态机等
//! 逻辑链结构及可视化高亮数据（全链路/光线/波纹/着色/蛛网/X光/时间切片/对比叠加）。

use crate::model::{EdgeKind, ProjectIR, StmtKind};
use std::collections::HashMap;

// F038 全量调用图 —— 跨文件调用边聚合 + 虚函数（间接调用）标记
pub struct CallGraph {
    pub edges: Vec<(String, String)>,
    pub virtual_calls: Vec<String>,
}

pub fn build_call_graph(ir: &ProjectIR) -> CallGraph {
    let mut g = CallGraph { edges: Vec::new(), virtual_calls: Vec::new() };
    for e in &ir.calls {
        if e.kind != EdgeKind::Call {
            continue;
        }
        if !g.edges.iter().any(|(a, b)| a == &e.from && b == &e.to) {
            g.edges.push((e.from.clone(), e.to.clone()));
        }
        // 含 ".method(" 形态或接口前缀视为虚调用（去虚化待 W4 C02）
        if e.to.contains('.') {
            g.virtual_calls.push(e.to.clone());
        }
    }
    g
}

// F039 SSA Def-Use链 —— 变量定义点 → 使用点 有向边
pub fn def_use_lines(stmts: &[(usize, StmtKind, Option<String>, Option<String>)]) -> Vec<(String, usize, usize)> {
    // (变量, def行, use行)
    let mut defs: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    let mut vars: Vec<(String, usize)> = Vec::new();
    for (line, kind, target, text) in stmts {
        let _ = kind;
        if let Some(t) = target {
            vars.push((t.clone(), *line));
        }
        if let Some(txt) = text {
            for (v, dline) in vars.clone() {
                if *line > dline && txt.contains(&v) {
                    let fresh_def = target.as_deref() == Some(v.as_str());
                    if fresh_def {
                        defs.insert(v.clone(), *line);
                    } else if !out.iter().any(|(vv, dd, _)| vv == &v && dd == &dline) {
                        out.push((v, dline, *line));
                    }
                }
            }
        }
    }
    let _ = defs;
    out
}

// F040 控制流图CFG —— 基本块 + 条件边 + 循环回边
pub struct Cfg {
    pub blocks: Vec<(usize, usize)>, // (start,end) 行区间
    pub edges: Vec<(usize, usize, &'static str)>,
}

pub fn build_cfg(stmt_kinds: &[StmtKind]) -> Cfg {
    let mut blocks = Vec::new();
    let mut edges = Vec::new();
    let mut leader = 0usize;
    for (i, k) in stmt_kinds.iter().enumerate() {
        let is_branch = matches!(k, StmtKind::If | StmtKind::Loop | StmtKind::Switch | StmtKind::Return);
        if is_branch {
            blocks.push((leader, i + 1));
            let b = blocks.len() - 1;
            match k {
                StmtKind::If | StmtKind::Switch => {
                    edges.push((b, b + 1, "true"));
                    edges.push((b, b + 2, "false"));
                }
                StmtKind::Loop => edges.push((b, leader.min(b), "back")),
                StmtKind::Return => edges.push((b, usize::MAX, "exit")),
                _ => {}
            }
            leader = i + 1;
        }
    }
    if leader < stmt_kinds.len() {
        blocks.push((leader, stmt_kinds.len()));
    }
    Cfg { blocks, edges }
}

// F041 异常传播路径 —— throw → 最近 catch，含未捕获路径
pub fn exception_paths(stmts: &[(usize, StmtKind, Option<String>)]) -> Vec<(usize, Option<usize>)> {
    let mut out = Vec::new();
    let mut open_try: Option<usize> = None;
    let mut catch_of: HashMap<usize, usize> = HashMap::new();
    for (l, k, _) in stmts.iter() {
        let (l, k) = (*l, *k);
        match k {
            StmtKind::Try => open_try = Some(l),
            StmtKind::Catch => {
                if let Some(t) = open_try.take() {
                    catch_of.insert(t, l);
                }
            }
            _ => {}
        }
    }
    for (l, k, _) in stmts {
        if *k == StmtKind::Throw {
            out.push((*l, catch_of.values().next().copied()));
        }
    }
    out
}

// F042 副作用依赖图 —— 函数对全局/堆/IO 的读写依赖
pub fn side_effects(func_body: &[String]) -> Vec<&'static str> {
    let mut eff = Vec::new();
    for l in func_body {
        let t = l.trim();
        for (pat, tag) in [
            ("print(", "io.write"),
            ("write(", "heap.write"),
            ("read(", "io.read"),
            ("global ", "global.rw"),
            ("new ", "heap.alloc"),
            ("free(", "heap.free"),
            ("open(", "fs.io"),
        ] {
            if t.contains(pat) && !eff.contains(&tag) {
                eff.push(tag);
            }
        }
    }
    eff
}

// F043 递归展开树 —— 按深度展开，标注终止条件命中点
pub fn expand_recursion(fname: &str, max_depth: usize, base_at: usize) -> Vec<(String, usize, bool)> {
    // 返回 (节点路径, 深度, 是否叶子/终止命中)
    let mut out = Vec::new();
    for d in 0..max_depth {
        let path = (0..d).fold(fname.to_string(), |acc, _| format!("{acc}.{fname}"));
        let leaf = d == base_at || d == max_depth - 1;
        out.push((path, d, leaf));
    }
    out
}

// F044 并发时序图 —— happens-before 时序（spawn/lock 事件排序）
pub fn happens_before(events: &[(usize, StmtKind, u32)]) -> Vec<(u32, u32)> {
    // 同一 lock 区间内 spawn 的线程对锁主线程建立 HB；简单模型：按事件顺序对相邻线程建立 HB
    let mut last_thread = 0u32;
    let mut out = Vec::new();
    for (_, k, th) in events {
        if *k == StmtKind::Lock {
            last_thread = *th;
        }
        if *k == StmtKind::Spawn && *th != last_thread {
            let pair = (last_thread, *th);
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

// F045 对象生命周期线 —— new → free 存活区间
pub fn lifetimes(stmts: &[(usize, StmtKind, Option<String>)]) -> Vec<(String, usize, usize)> {
    let mut birth: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for (l, k, tgt) in stmts.iter() {
        let (l, k, tgt) = (*l, *k, tgt);
        match k {
            StmtKind::New => {
                if let Some(t) = tgt {
                    birth.insert(t.clone(), l);
                }
            }
            StmtKind::Free => {
                if let Some(t) = tgt {
                    if let Some(b) = birth.remove(t) {
                        out.push((t.clone(), b, l));
                    }
                }
            }
            _ => {}
        }
    }
    // 未释放：区间开放到末行 -1
    for (t, b) in birth {
        out.push((t, b, usize::MAX));
    }
    out
}

// F046 隐式状态机提取 —— switch/case + 赋值模式 → 有限状态机
pub fn extract_fsm(stmts: &[(usize, StmtKind, Option<String>, Option<String>)]) -> Vec<(String, String)> {
    // 返回 (源状态, 目标状态) 转移表：case N: state = M
    let mut cur_state = String::from("init");
    let mut out = Vec::new();
    for (_, k, tgt, text) in stmts {
        match k {
            StmtKind::Case => {
                if let Some(t) = text {
                    cur_state = t.trim_end_matches(':').trim().to_string();
                }
            }
            StmtKind::Assign => {
                if let (Some(t), Some(tx)) = (tgt, text) {
                    if t.contains("state") {
                        if let Some(v) = tx.split('=').nth(1) {
                            let to = v.trim().trim_matches(';').to_string();
                            if !out.iter().any(|(a, b)| a == &cur_state && b == &to) {
                                out.push((cur_state.clone(), to));
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

// F047 模块依赖拓扑排序 —— DAG 拓扑 + 循环依赖检测
pub fn toposort(deps: &[(String, String)]) -> (Vec<String>, Vec<Vec<String>>) {
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (a, b) in deps {
        *indeg.entry(b.as_str()).or_default() += 1;
        indeg.entry(a.as_str()).or_default();
        adj.entry(a.as_str()).or_default().push(b.as_str());
    }
    let mut q: Vec<&str> = indeg.iter().filter(|(_, d)| **d == 0).map(|(k, _)| *k).collect();
    q.sort();
    let mut order = Vec::new();
    let mut i = 0;
    while i < q.len() {
        let n = q[i];
        i += 1;
        order.push(n.to_string());
        if let Some(nb) = adj.get(n) {
            for m in nb.clone() {
                let e = indeg.get_mut(m).unwrap();
                *e -= 1;
                if *e == 0 {
                    q.push(m);
                }
            }
        }
    }
    let cycles: Vec<Vec<String>> = if order.len() < indeg.len() {
        vec![indeg
            .keys()
            .filter(|k| !order.iter().any(|o| o == **k))
            .map(|k| k.to_string())
            .collect()]
    } else {
        Vec::new()
    };
    (order, cycles)
}

// F048 数据流图DFG —— 真依赖/反依赖/输出依赖
#[derive(Debug, PartialEq, Eq)]
pub enum DepKind {
    True,
    Anti,
    Output,
}

pub fn dfg(stmts: &[(usize, Option<String>, Vec<String>)]) -> Vec<(String, usize, usize, DepKind)> {
    // (变量, 写行 i, 写/读行 j, 依赖类型)
    let mut out: Vec<(String, usize, usize, DepKind)> = Vec::new();
    for (i, st1) in stmts.iter().enumerate() {
        for (j, st2) in stmts.iter().enumerate().skip(i + 1) {
            let (wr, reads) = (&st1.1, &st1.2);
            let (wj, rj) = (&st2.1, &st2.2);
            let _ = wj;
            for v in reads {
                if wj.as_deref() == Some(v.as_str()) {
                    // j 写 v，i 读 v：反依赖（WAR）
                    push(&mut out, v.clone(), i, j, DepKind::Anti);
                }
            }
            for v in rj {
                if wr.as_deref() == Some(v.as_str()) {
                    // i 写 v，j 读 v：真依赖（RAW）
                    push(&mut out, v.clone(), i, j, DepKind::True);
                }
            }
            if let (Some(a), Some(b)) = (wr.as_deref(), wj.as_deref()) {
                if a == b {
                    push(&mut out, a.to_string(), i, j, DepKind::Output);
                }
            }
        }
    }
    out
}

fn push(v: &mut Vec<(String, usize, usize, DepKind)>, s: String, i: usize, j: usize, k: DepKind) {
    if !v.iter().any(|(a, b, c, d)| a == &s && b == &i && c == &j && *d == k) {
        v.push((s, i, j, k));
    }
}

// F049 指针逃逸链 —— 从创建到逃逸出函数的路径
pub fn escape_chain(stmts: &[(usize, StmtKind, Option<String>)]) -> Vec<(String, usize, bool)> {
    // (对象, 创建行, 是否逃逸：被 return/全局赋值/global 关键字触达)
    let mut out = Vec::new();
    let ret_lines: Vec<usize> = stmts.iter().filter(|(_, k, _)| *k == StmtKind::Return).map(|(l, _, _)| *l).collect();
    for (l, k, tgt) in stmts {
        if *k == StmtKind::New {
            if let Some(t) = tgt {
                let escaped = ret_lines.iter().any(|&r| {
                    stmts
                        .iter()
                        .filter(|(l2, k2, _)| *l2 >= *l && *l2 <= r && *k2 == StmtKind::Assign)
                        .any(|(_, _, tx)| tx.as_deref().map_or(false, |x| x.contains(t.as_str())))
                }) || stmts.iter().any(|(l2, k2, tx)| {
                    *l2 > *l && *k2 == StmtKind::Assign && tx.as_deref().map_or(false, |x| x.contains("global"))
                });
                out.push((t.clone(), *l, escaped));
            }
        }
    }
    out
}

// F050 闭包捕获链 —— 捕获的外部变量 + 值/引用
pub fn closure_captures(body: &[String], outer: &[String]) -> Vec<(String, bool)> {
    // true = 引用捕获（体内有赋值），false = 值捕获
    outer
        .iter()
        .filter(|v| body.iter().any(|l| l.contains(v.as_str())))
        .map(|v| {
            let by_ref = body
                .iter()
                .any(|l| l.trim_start().starts_with(&format!("{v} =")));
            (v.clone(), by_ref)
        })
        .collect()
}

// F051 事件驱动链 —— emit → on 跨模块事件流配对
pub fn event_chains(ir: &ProjectIR) -> Vec<(String, String)> {
    let mut emitters: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut listeners: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &ir.events {
        if e.kind == EdgeKind::Event {
            emitters.entry(e.from.as_str()).or_default().push(e.to.as_str());
            listeners.entry(e.to.as_str()).or_default().push(e.from.as_str());
        }
    }
    let mut out = Vec::new();
    for e in &ir.events {
        if e.kind == EdgeKind::Event && listeners.contains_key(e.to.as_str()) {
            let pair = (e.from.clone(), e.to.clone());
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    let _ = emitters;
    out
}

// F052 回调地狱拉平 —— 嵌套回调/async 链展平为线性步骤
pub fn flatten_callbacks(nested: &[String]) -> Vec<String> {
    // 输入形如 ["a(function(){", "b(function(){", "c()", "})", "})"] → [a, b, c]
    let mut steps = Vec::new();
    for l in nested {
        let t = l.trim().trim_end_matches(|c| c == '{' || c == '}' || c == ';');
        if t.chars().all(|c| !(c.is_alphanumeric() || c == '_')) {
            continue; // 纯括号/分号收口行不是步骤
        }
        let head = t
            .split(|c: char| c == '(')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !head.is_empty() && !head.starts_with("function") {
            if !steps.contains(&head) {
                steps.push(head);
            }
        }
    }
    steps
}

// F053 宏展开链 —— 递归展开显示每层中间结果
pub fn expand_macros(rules: &HashMap<String, String>, mut expr: String, max_layers: usize) -> Vec<String> {
    let mut layers = vec![expr.clone()];
    for _ in 0..max_layers {
        // 每层只展开一条规则：MAX → LIMIT 是一层，LIMIT → 100 是下一层
        let hit = rules.iter().find(|(k, _)| expr.contains(k.as_str()));
        match hit {
            Some((k, v)) => {
                expr = expr.replace(k.as_str(), v.as_str());
                layers.push(expr.clone());
            }
            None => break,
        }
    }
    layers
}

// F054 泛型实例化链 —— 泛型定义 → 具体实例的类型替换路径
pub fn instantiate_generic(def: &str, subs: &[(&str, &str)]) -> Vec<String> {
    let mut cur = def.to_string();
    let mut path = vec![cur.clone()];
    for (from, to) in subs {
        cur = cur.replace(from, to);
        path.push(cur.clone());
    }
    path
}

// F055 指针别名分析 —— Andersen 包含分析（union-find 简化）
pub struct AliasSet {
    parent: HashMap<String, String>,
}

impl AliasSet {
    pub fn new() -> Self {
        AliasSet { parent: HashMap::new() }
    }
    fn find(&mut self, x: &str) -> String {
        let x = x.to_string();
        let p = self.parent.entry(x.clone()).or_insert_with(|| x.clone()).clone();
        if p == x {
            x
        } else {
            let r = self.find(&p);
            self.parent.insert(x, r.clone());
            r
        }
    }
    /// p = &q 约束：p 可能指向 q。
    pub fn points_to(&mut self, p: &str, q: &str) {
        let rp = self.find(p);
        let rq = self.find(q);
        if rp != rq {
            self.parent.insert(rp, rq);
        }
    }
    pub fn may_alias(&mut self, a: &str, b: &str) -> bool {
        self.find(a) == self.find(b)
    }
}

// F056 类型层次图 —— 继承/实现的偏序 Hasse 图
pub fn type_hierarchy(rels: &[(String, String)]) -> Vec<(String, String, usize)> {
    // (子, 父, 层深)：父在顶层深度 0
    let mut depth: HashMap<String, usize> = HashMap::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (c, p) in rels {
            let dp = *depth.get(p).unwrap_or(&0);
            let dc = depth.get(c).copied();
            if dc != Some(dp + 1) {
                depth.insert(c.clone(), dp + 1);
                changed = true;
            }
        }
    }
    rels.iter()
        .map(|(c, p)| (c.clone(), p.clone(), *depth.get(c).unwrap_or(&1)))
        .collect()
}

// F057 全链路高亮 —— 入口到出口完整路径
pub fn full_path(graph: &[(String, String)], entry: &str) -> Vec<String> {
    let mut path = vec![entry.to_string()];
    let mut cur = entry.to_string();
    loop {
        let next = graph.iter().find(|(a, _)| a == &cur).map(|(_, b)| b.clone());
        match next {
            Some(n) if !path.contains(&n) => {
                path.push(n.clone());
                cur = n;
            }
            _ => break,
        }
    }
    path
}

// F058 变量追踪光线 —— 变量流经的所有节点（def-use 链投影）
pub fn trace_variable(du: &[(String, usize, usize)], var: &str) -> Vec<usize> {
    let mut lines: Vec<usize> = Vec::new();
    for (v, d, u) in du {
        if v == var {
            if !lines.contains(d) {
                lines.push(*d);
            }
            if !lines.contains(u) {
                lines.push(*u);
            }
        }
    }
    lines.sort();
    lines
}

// F059 异常传播波纹 —— throw 沿路径扩散到 catch 的节点序列
pub fn exception_ripple(paths: &[(usize, Option<usize>)], throw_at: usize) -> Vec<usize> {
    let mut out = vec![throw_at];
    if let Some((_, Some(c))) = paths.iter().find(|(t, _)| *t == throw_at) {
        out.push(*c);
    }
    out
}

// F060 并发线程着色 —— 每线程一色（色号 = 稳定哈希）
pub fn thread_colors(threads: &[u32]) -> HashMap<u32, u32> {
    threads
        .iter()
        .map(|&t| {
            let mut h = t as u64 ^ 0x9e3779b97f4a7c15;
            h ^= h >> 33;
            h = h.wrapping_mul(0xff51afd7ed558ccd);
            h ^= h >> 33;
            (t, (h % 360) as u32) // 色相
        })
        .collect()
}

// F061 依赖蛛网 —— 选中模块的所有直接+传递依赖边
pub fn dep_web(deps: &[(String, String)], selected: &str) -> Vec<(String, String)> {
    let mut seen = vec![selected.to_string()];
    let mut out = Vec::new();
    let mut i = 0;
    while i < seen.len() {
        let cur = seen[i].clone();
        i += 1;
        for (a, b) in deps {
            if a == &cur {
                if !out.contains(&(a.clone(), b.clone())) {
                    out.push((a.clone(), b.clone()));
                }
                if !seen.contains(b) {
                    seen.push(b.clone());
                }
            }
        }
    }
    out
}

// F062 逻辑X光 —— 隐藏装饰，只留纯逻辑骨架（控制流/返回/调用）
pub fn xray(stmts: &[(usize, StmtKind, Option<String>)]) -> Vec<(usize, &'static str)> {
    stmts
        .iter()
        .filter(|(_, k, _)| {
            matches!(
                k,
                StmtKind::If | StmtKind::Loop | StmtKind::Return | StmtKind::Call | StmtKind::Switch
            )
        })
        .map(|(l, k, _)| (*l, kind_name(*k)))
        .collect()
}

fn kind_name(k: StmtKind) -> &'static str {
    match k {
        StmtKind::If => "if",
        StmtKind::Loop => "loop",
        StmtKind::Return => "return",
        StmtKind::Call => "call",
        StmtKind::Switch => "switch",
        _ => "other",
    }
}

// F063 时间切片 —— 拖动时间轴显示该时刻执行状态
pub fn time_slice(trace: &[(usize, String)], t: usize) -> Option<&String> {
    trace.iter().find(|(ts, _)| *ts == t).map(|(_, s)| s)
}

// F064 对比叠加 —— 两版本差异：新增/删除行分类
pub fn overlay(old: &[String], new: &[String]) -> Vec<(usize, &'static str, String)> {
    let mut out = Vec::new();
    for (i, l) in old.iter().enumerate() {
        if !new.contains(l) {
            out.push((i, "del", l.clone()));
        }
    }
    for (i, l) in new.iter().enumerate() {
        if !old.contains(l) {
            out.push((i, "add", l.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, EdgeKind};

    fn edge(a: &str, b: &str, k: EdgeKind) -> Edge {
        Edge { from: a.into(), to: b.into(), kind: k }
    }

    #[test]
    fn f038_call_graph_and_virtual() {
        let ir = ProjectIR {
            calls: vec![edge("main", "f", EdgeKind::Call), edge("main", "Shape.draw", EdgeKind::Call)],
            ..Default::default()
        };
        let g = build_call_graph(&ir);
        assert_eq!(g.edges.len(), 2);
        assert!(g.virtual_calls.contains(&"Shape.draw".to_string()));
    }

    #[test]
    fn f039_def_use_chain() {
        let st = vec![
            (1, StmtKind::Assign, Some("x".into()), Some("x = 1".to_string())),
            (2, StmtKind::Call, None, Some("use(x)".to_string())),
        ];
        let du = def_use_lines(&st);
        assert!(du.contains(&("x".into(), 1, 2)));
    }

    #[test]
    fn f040_cfg_branch_back_exit() {
        let ks = [StmtKind::Assign, StmtKind::If, StmtKind::Loop, StmtKind::Return];
        let cfg = build_cfg(&ks);
        assert!(cfg.edges.iter().any(|e| e.2 == "true"));
        assert!(cfg.edges.iter().any(|e| e.2 == "false"));
        assert!(cfg.edges.iter().any(|e| e.2 == "back"));
        assert!(cfg.edges.iter().any(|e| e.2 == "exit"));
    }

    #[test]
    fn f041_exception_paths_uncaught() {
        let st = vec![(1, StmtKind::Throw, None), (3, StmtKind::Throw, None)];
        let p = exception_paths(&st);
        assert_eq!(p.len(), 2);
        assert!(p.iter().all(|(_, c)| c.is_none()));
    }

    #[test]
    fn f042_side_effects() {
        let e = side_effects(&["print(x)".into(), "global cnt = 1".into(), "y = read(f)".into()]);
        assert!(e.contains(&"io.write"));
        assert!(e.contains(&"global.rw"));
        assert!(e.contains(&"io.read"));
    }

    #[test]
    fn f043_recursion_expansion() {
        let t = expand_recursion("fact", 4, 2);
        assert_eq!(t.len(), 4);
        assert!(t[2].2); // 终止条件命中
        assert!(t[0].0.contains("fact"));
    }

    #[test]
    fn f044_happens_before() {
        let ev = vec![(1, StmtKind::Lock, 0), (2, StmtKind::Spawn, 1)];
        let hb = happens_before(&ev);
        assert_eq!(hb, vec![(0, 1)]);
    }

    #[test]
    fn f045_lifetimes_open_and_closed() {
        let st = vec![
            (1, StmtKind::New, Some("p".into())),
            (4, StmtKind::Free, Some("p".into())),
            (5, StmtKind::New, Some("q".into())),
        ];
        let lt = lifetimes(&st);
        assert!(lt.contains(&("p".into(), 1, 4)));
        assert!(lt.contains(&("q".into(), 5, usize::MAX)));
    }

    #[test]
    fn f046_fsm_extraction() {
        let st = vec![
            (1, StmtKind::Case, None, Some("Idle:".to_string())),
            (2, StmtKind::Assign, Some("state".into()), Some("state = Run".to_string())),
            (3, StmtKind::Case, None, Some("Run:".to_string())),
            (4, StmtKind::Assign, Some("state".into()), Some("state = Idle".to_string())),
        ];
        let fsm = extract_fsm(&st);
        assert!(fsm.contains(&("Idle".into(), "Run".into())));
        assert!(fsm.contains(&("Run".into(), "Idle".into())));
    }

    #[test]
    fn f047_toposort_and_cycle() {
        let deps = vec![("app".to_string(), "net".to_string()), ("net".to_string(), "core".to_string())];
        let (order, cyc) = toposort(&deps);
        assert_eq!(order, vec!["app", "net", "core"]);
        assert!(cyc.is_empty());
        let deps2 = vec![("a".to_string(), "b".to_string()), ("b".to_string(), "a".to_string())];
        let (_, cyc2) = toposort(&deps2);
        assert!(!cyc2.is_empty());
    }

    #[test]
    fn f048_dfg_three_kinds() {
        let st = vec![
            (0, Some("a".into()), vec!["b".to_string()]), // 读 b
            (1, Some("b".into()), vec!["a".to_string()]), // 写 b（a 的反依赖），读 a（真依赖）
            (2, Some("a".into()), vec![]),                // 再写 a（输出依赖）
        ];
        let d = dfg(&st);
        assert!(d.contains(&("a".to_string(), 0, 1, DepKind::True)));
        assert!(d.contains(&("b".to_string(), 0, 1, DepKind::Anti)));
        assert!(d.contains(&("a".to_string(), 0, 2, DepKind::Output)));
    }

    #[test]
    fn f049_escape_analysis() {
        let st = vec![
            (1, StmtKind::New, Some("p".into())),
            (2, StmtKind::Assign, Some("r = p".into())),
            (3, StmtKind::Return, None),
        ];
        let e = escape_chain(&st);
        assert!(e.contains(&("p".into(), 1, true)));
    }

    #[test]
    fn f050_closure_capture_ref_vs_value() {
        let body = vec!["count = count + 1".into(), "log(count)".into()];
        let caps = closure_captures(&body, &["count".to_string(), "other".to_string()]);
        assert_eq!(caps, vec![("count".into(), true)]);
    }

    #[test]
    fn f051_event_chains() {
        let ir = ProjectIR {
            events: vec![edge("ui", "click", EdgeKind::Event), edge("ctrl", "click", EdgeKind::Event)],
            ..Default::default()
        };
        let c = event_chains(&ir);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn f052_flatten_callbacks() {
        let nested = vec![
            "a(function(){".to_string(),
            "  b(function(){".to_string(),
            "    c();".to_string(),
            "  })".to_string(),
            "})".to_string(),
        ];
        assert_eq!(flatten_callbacks(&nested), vec!["a", "b", "c"]);
    }

    #[test]
    fn f053_macro_layers() {
        let mut rules = HashMap::new();
        rules.insert("MAX".to_string(), "LIMIT".to_string());
        rules.insert("LIMIT".to_string(), "100".to_string());
        let l = expand_macros(&rules, "MAX + 1".into(), 5);
        assert_eq!(l, vec!["MAX + 1", "LIMIT + 1", "100 + 1"]);
    }

    #[test]
    fn f054_generic_instantiation() {
        let p = instantiate_generic("Vec<T> push(t: T)", &[("T", "int")]);
        assert_eq!(p, vec!["Vec<T> push(t: T)", "Vec<int> push(t: int)"]);
    }

    #[test]
    fn f055_andersen_alias() {
        let mut a = AliasSet::new();
        a.points_to("p", "x");
        a.points_to("q", "x");
        assert!(a.may_alias("p", "q"));
        assert!(!a.may_alias("p", "y"));
    }

    #[test]
    fn f056_hierarchy_depths() {
        let r = vec![
            ("Dog".to_string(), "Animal".to_string()),
            ("Puppy".to_string(), "Dog".to_string()),
        ];
        let h = type_hierarchy(&r);
        assert!(h.contains(&("Puppy".into(), "Dog".into(), 2)));
        assert!(h.contains(&("Dog".into(), "Animal".into(), 1)));
    }

    #[test]
    fn f057_full_path_highlight() {
        let g = vec![("main".to_string(), "f".to_string()), ("f".to_string(), "g".to_string())];
        assert_eq!(full_path(&g, "main"), vec!["main", "f", "g"]);
    }

    #[test]
    fn f058_trace_variable_rays() {
        let du = vec![("x".to_string(), 1, 3), ("x".to_string(), 3, 7), ("y".to_string(), 2, 4)];
        assert_eq!(trace_variable(&du, "x"), vec![1, 3, 7]);
    }

    #[test]
    fn f059_ripple_to_catch() {
        let paths = vec![(5, Some(9))];
        assert_eq!(exception_ripple(&paths, 5), vec![5, 9]);
    }

    #[test]
    fn f060_thread_colors_distinct() {
        let c = thread_colors(&[0, 1, 2]);
        assert_eq!(c.len(), 3);
        assert_ne!(c[&0], c[&1]);
    }

    #[test]
    fn f061_dep_web_transitive() {
        let deps = vec![
            ("app".to_string(), "net".to_string()),
            ("net".to_string(), "core".to_string()),
            ("x".to_string(), "y".to_string()),
        ];
        let w = dep_web(&deps, "app");
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn f062_xray_skeleton_only() {
        let st = vec![
            (1, StmtKind::Assign, Some("a".to_string().into())),
            (2, StmtKind::If, None),
            (3, StmtKind::Return, None),
        ];
        let x = xray(&st);
        assert_eq!(x, vec![(2, "if"), (3, "return")]);
    }

    #[test]
    fn f063_time_slice_lookup() {
        let tr = vec![(0, "start".to_string()), (5, "in-loop".to_string())];
        assert_eq!(time_slice(&tr, 5), Some(&"in-loop".to_string()));
        assert_eq!(time_slice(&tr, 3), None);
    }

    #[test]
    fn f064_overlay_add_del() {
        let old = vec!["a".to_string(), "b".to_string()];
        let new = vec!["a".to_string(), "c".to_string()];
        let ov = overlay(&old, &new);
        assert!(ov.contains(&(1, "del", "b".to_string())));
        assert!(ov.contains(&(1, "add", "c".to_string())));
    }
}
