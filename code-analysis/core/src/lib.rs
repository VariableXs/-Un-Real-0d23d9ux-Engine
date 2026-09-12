//! (Un)Real 0d23d9ux# Engine core —— W1 地基（#001~#064）。
//!
//! 零 AI：全程确定性算法，不调用任何网络模型。
//! 一域一文件：infra（#001~#017）/ cont（#018~#037）/ logic（#038~#064），
//! 每域 CheckSet 自检逐项断言，`run_ca_checks()` 汇总。

pub mod canvas;
pub mod checks;
pub mod cont;
pub mod dyna;
pub mod flowchart;
pub mod iface;
pub mod infra;
pub mod ir;
pub mod langid;
pub mod logic;
pub mod model;
pub mod nouns;
pub mod parser;
pub mod refactor;
pub mod roottree;
pub mod simple;
pub mod statics;
pub mod translate;

use checks::CheckSet;

pub fn run_infra_checks() -> CheckSet {
    let mut s = CheckSet::new("infra");
    let src = "a = 1\nb = 2\nfn f(x) {\n  return x\n}\n";
    let mut ls = infra::LazySource::open(src, 2);
    let l0 = ls.line(0).len() > 0 && ls.page_faults == 1;
    let again = ls.line(1);
    s.add("F001 mmap零拷贝加载", l0 && again.len() > 0 && ls.page_faults == 1 && ls.loaded_pages() >= 1, "按需分页/缺页计数");
    let old = parser::parse_file("t.rs", src);
    let inc = infra::parse_incremental(&old, src, 1);
    s.add("F002 增量AST解析", inc.node(inc.root).kind == model::NodeKind::File, "只重解析编辑子树");
    let sh = infra::parse_sharded(src, 2);
    let nf = sh.nodes.iter().filter(|n| n.kind == model::NodeKind::Func).count();
    s.add("F003 文件分片并行解析", nf == 1, "分片构建后合并");
    let mut fp = infra::FingerprintIndex::new();
    fp.index("f", &["return x".into()]);
    fp.index("g", &["return x".into()]);
    fp.index("h", &["return y".into()]);
    s.add("F004 语法指纹索引", fp.duplicates().len() == 1, "同指纹判重");
    let mut sym = infra::LazySymbols::new(vec![vec![("a".into(), String::new())]]);
    let r1 = sym.lookup("a").map(|(_, fresh)| fresh).unwrap_or(false);
    let r2 = sym.lookup("a").map(|(_, fresh)| fresh).unwrap_or(true);
    s.add("F005 符号表懒加载", r1 && !r2, "首次查询才解析");
    let mut va = infra::VirtualAst::new(100);
    let m = va.scroll(10, 20, 5);
    s.add("F006 AST虚拟化按需物化", m.len() == 20 && va.resident() == 20, "可视区±缓冲");
    let calls = vec![model::Edge { from: "main".into(), to: "f".into(), kind: model::EdgeKind::Call }];
    let imp = infra::impact_scope(&calls, "f");
    s.add("F007 变更影响域计算", imp.contains(&"main".to_string()), "反向依赖遍历");
    let mut db = infra::SymbolDb::default();
    db.define("open", "net.rs", 1);
    s.add("F008 跨文件符号数据库", db.query("open").len() == 1 && db.query("x").is_empty(), "项目级符号索引");
    s.add("F009 语法岛检测", infra::detect_island("\"SELECT 1\"") == infra::Island::Sql, "内嵌子语言识别");
    let mut hot = infra::Hotspots::new(2);
    hot.touch(3);
    hot.touch(3);
    hot.touch(9);
    s.add("F010 编辑热区检测", hot.hot() == vec![3], "热区常驻冷区换出");
    let mut fw = infra::FlyweightPool::new();
    let i1 = fw.intern("func", "f");
    let i2 = fw.intern("func", "f");
    s.add("F011 AST节点压缩存储", i1 == i2 && fw.saved == 1, "Flyweight 共享");
    let mut snap = infra::SnapshotTree::new();
    let v = snap.commit(0, "e1");
    s.add("F012 多版本AST快照树", snap.undo_path(v) == vec![1, 0], "版本链 undo");
    let bl = infra::split_blocks(src);
    s.add("F013 逻辑区块自动分割", bl.len() == 3, "顶层声明切块");
    let ifc = infra::stitch_iface(&["fn f(a) {".into(), "return a".into()]);
    s.add("F014 跨区块逻辑缝合", ifc.inputs == vec!["a"] && !ifc.outputs.is_empty(), "输入/输出/副作用摘要");
    s.add("F015 文件摘要自动生成", infra::file_summary(src).contains("fn f"), "区块摘要→目录");
    let mut an = infra::Anchors::default();
    an.place("k", 7);
    an.place("k", 9);
    s.add("F016 逻辑锚点书签", an.jump("k") == Some(7), "可跳转锚点+去重");
    let tree = parser::parse_file("m.rs", "mod n {\nfn a() {\n}\n}\n");
    let mac = infra::logical_map(&tree, infra::MapScale::Macro);
    let meso = infra::logical_map(&tree, infra::MapScale::Meso);
    s.add("F017 超大文件逻辑地图", mac.iter().any(|(n, _)| n == "n") && meso.iter().any(|(n, _)| n == "a"), "宏观/中观/微观三档");
    s
}

pub fn run_cont_checks() -> CheckSet {
    let mut s = CheckSet::new("cont");
    s.add("F018 合法Token枚举器", cont::legal_tokens(cont::Ctx::FileTop).contains(&"fn"), "文法产生式枚举");
    let env = vec![cont::TypedSym { name: "n".into(), ty: "int".into() }];
    s.add("F019 类型约束续写", cont::typed_completion(&env, "int") == vec!["n"] && cont::infer("1", &env) == "int", "类型匹配推荐");
    let chain = vec![env.clone(), vec![cont::TypedSym { name: "g".into(), ty: "int".into() }]];
    s.add("F020 作用域符号雷达", cont::scope_radar(&chain, Some("int"))[0].0 == "n", "按距离+类型排序");
    let sk = cont::stub_skeleton("fn c(a: int) -> bool");
    s.add("F021 API签名骨架填充", sk.starts_with("fn c(a)") && sk.contains("false"), "存根→占位骨架");
    let lib = cont::TemplateLib::builtin();
    s.add("F022 惯用法AST模板库", lib.match_fill("for loop", "it").is_some(), "模板匹配+填充");
    s.add("F023 对称结构闭合", cont::close_structure("if (a) {") == "if (a) {}", "确定性补全");
    let cs = cont::comment_skeleton(&["@param x".to_string().as_str(), "@return 0".to_string().as_str()], "f");
    s.add("F024 结构化注释→骨架", cs.starts_with("fn f(x)") && cs.contains("return 0"), "@param/@return 解析");
    let sig = cont::infer_from_tests(&["assert_eq!(add(1, 2), 3)".into()], "add");
    s.add("F025 测试断言反推", sig.contains("2 args, 1 cases"), "断言反推签名");
    let fx = cont::ImportFixer { index: [("o".to_string(), "m".to_string())].into_iter().collect() };
    s.add("F026 import自动修复", fx.fix("", &["o".to_string()]) == vec!["import o from m"], "未定义符号→import");
    let iso = cont::extrapolate_isomorphic(&["p1.d()".into(), "p2.d()".into(), "p3.d()".into()]);
    s.add("F027 AST同构模式外推", iso.as_deref() == Some("p4.d()"), "3+ 同构外推");
    s.add("F028 CYK语法补全", cont::cyk_complete(&["(", "1", "+"]) == "( 1 + 0 )", "最短合法补全");
    s.add("F029 接口约束推导续写", cont::interface_methods("trait T { a(); b(); }", &["a".to_string()]) == vec!["b"], "枚举未实现方法");
    let rec = cont::panic_recover(&["!!".to_string(), "}".to_string()], 0);
    s.add("F030 Panic Mode错误恢复", rec.0 == 1, "跳同步点恢复");
    let lib2 = vec![vec!["return a + b".to_string()]];
    s.add("F031 代码片段精确匹配", cont::snippet_match(&["return  a+b".into()], &lib2) == Some(0), "子树同构匹配");
    s.add("F032 正则→代码转换", cont::regex_to_code("cat|dog").starts_with("alts:"), "正则子集→等价代码");
    s.add("F033 DSL内嵌续写", cont::detect_dsl("SELECT 1") == cont::Dsl::Sql && !cont::dsl_next_token(cont::Dsl::Sql).is_empty(), "切换对应文法");
    s.add("F034 AST合法Token预测", cont::next_node_types("after-if").contains(&"else"), "残缺AST下一节点类型");
    let batch = cont::skeletons_from_comments(&[("a", vec![])]);
    s.add("F035 注释→骨架生成器", batch[0].starts_with("fn a()"), "批量骨架");
    s.add("F036 重复模式外推", cont::fill_pattern(&["v1".into()], 2) == vec!["v2", "v3"], "Excel 填充语义");
    s.add("F037 作用域变量联想", cont::scope_suggest(&chain) == vec!["n", "g"], "作用域链全符号");
    s
}

pub fn run_logic_checks() -> CheckSet {
    let mut s = CheckSet::new("logic");
    use logic::*;
    let ir = model::ProjectIR {
        calls: vec![model::Edge { from: "m".into(), to: "f".into(), kind: model::EdgeKind::Call }],
        events: vec![model::Edge { from: "ui".into(), to: "clk".into(), kind: model::EdgeKind::Event }],
        ..Default::default()
    };
    let g = build_call_graph(&ir);
    s.add("F038 全量调用图", g.edges.len() == 1 && g.virtual_calls.is_empty(), "CHA/RTA + 虚调用标记");
    let du = def_use_lines(&[(1, model::StmtKind::Assign, Some("x".into()), Some("x = 1".to_string())), (2, model::StmtKind::Call, None, Some("use(x)".to_string()))]);
    s.add("F039 SSA Def-Use链", du.contains(&("x".into(), 1, 2)), "def→use 有向边");
    let cfg = build_cfg(&[model::StmtKind::If, model::StmtKind::Loop, model::StmtKind::Return]);
    s.add("F040 控制流图CFG", cfg.edges.iter().any(|e| e.2 == "back") && cfg.edges.iter().any(|e| e.2 == "exit"), "基本块+回边+异常边");
    s.add("F041 异常传播路径", exception_paths(&[(1, model::StmtKind::Throw, None)]).len() == 1, "throw→catch 含未捕获");
    s.add("F042 副作用依赖图", side_effects(&["print(x)".into()]) == vec!["io.write"], "全局/堆/IO 读写依赖");
    s.add("F043 递归展开树", expand_recursion("f", 3, 1).iter().filter(|(_, _, leaf)| *leaf).count() >= 1, "深度展开+终止标注");
    s.add("F044 并发时序图", happens_before(&[(1, model::StmtKind::Lock, 0), (2, model::StmtKind::Spawn, 1)]) == vec![(0, 1)], "happens-before 边");
    let lt = lifetimes(&[(1, model::StmtKind::New, Some("p".into())), (4, model::StmtKind::Free, Some("p".into()))]);
    s.add("F045 对象生命周期线", lt.contains(&("p".into(), 1, 4)), "new→free 区间");
    let fsm = extract_fsm(&[(1, model::StmtKind::Case, None, Some("A:".to_string())), (2, model::StmtKind::Assign, Some("state".into()), Some("state = B".to_string()))]);
    s.add("F046 隐式状态机提取", fsm.contains(&("A".into(), "B".into())), "switch/赋值模式→FSM");
    let (order, cyc) = toposort(&[("a".to_string(), "b".to_string())]);
    s.add("F047 模块依赖拓扑排序", order == vec!["a", "b"] && cyc.is_empty(), "DAG 拓扑+环检测");
    let d = dfg(&[(0, Some("a".into()), vec![]), (1, None, vec!["a".to_string()])]);
    s.add("F048 数据流图DFG", d.contains(&("a".to_string(), 0, 1, DepKind::True)), "真/反/输出依赖");
    let esc = escape_chain(&[(1, model::StmtKind::New, Some("p".into())), (2, model::StmtKind::Assign, Some("r = p".into())), (3, model::StmtKind::Return, None)]);
    s.add("F049 指针逃逸链", esc.contains(&("p".into(), 1, true)), "创建→逃逸路径");
    s.add("F050 闭包捕获链", closure_captures(&["c = c+1".into()], &["c".to_string()]) == vec![("c".into(), true)], "值/引用捕获");
    s.add("F051 事件驱动链", event_chains(&ir).contains(&("ui".into(), "clk".into())), "emit→on 配对");
    let fl = flatten_callbacks(&["a(function(){".to_string(), "b()".to_string(), "})".to_string()]);
    s.add("F052 回调地狱拉平", fl == vec!["a", "b"], "嵌套→线性步骤");
    let mut rules = std::collections::HashMap::new();
    rules.insert("M".to_string(), "1".to_string());
    s.add("F053 宏展开链", expand_macros(&rules, "M".into(), 3) == vec!["M", "1"], "逐层中间结果");
    s.add("F054 泛型实例化链", instantiate_generic("V<T>", &[("T", "i32")]).last().map(|s| s.as_str()) == Some("V<i32>"), "类型替换路径");
    let mut al = AliasSet::new();
    al.points_to("p", "x");
    al.points_to("q", "x");
    s.add("F055 指针别名分析", al.may_alias("p", "q") && !al.may_alias("p", "z"), "Andersen 包含分析");
    let h = type_hierarchy(&[("C".to_string(), "P".to_string())]);
    s.add("F056 类型层次图", h.contains(&("C".into(), "P".into(), 1)), "Hasse 偏序图");
    let path = full_path(&[("a".to_string(), "b".to_string()), ("b".to_string(), "c".to_string())], "a");
    s.add("F057 全链路高亮", path == vec!["a", "b", "c"], "入口→出口路径");
    s.add("F058 变量追踪光线", trace_variable(&du, "x") == vec![1, 2], "def-use 投影");
    s.add("F059 异常传播波纹", exception_ripple(&[(1, Some(2))], 1) == vec![1, 2], "throw→catch 扩散");
    let tc = thread_colors(&[0, 1]);
    s.add("F060 并发线程着色", tc.len() == 2 && tc[&0] != tc[&1], "线程→稳定色相");
    s.add("F061 依赖蛛网", dep_web(&[("a".to_string(), "b".to_string())], "a") == vec![("a".into(), "b".into())], "直接+传递依赖边");
    let x = xray(&[(1, model::StmtKind::Assign, None), (2, model::StmtKind::Return, None)]);
    s.add("F062 逻辑X光", x == vec![(2, "return")], "纯逻辑骨架");
    let tr = vec![(0, "s".to_string()), (9, "e".to_string())];
    s.add("F063 时间切片", time_slice(&tr, 9) == Some(&"e".to_string()) && time_slice(&tr, 5).is_none(), "该时刻执行状态");
    let ov = overlay(&["a".to_string()], &["b".to_string()]);
    s.add("F064 对比叠加", ov.contains(&(0, "del", "a".to_string())) && ov.contains(&(0, "add", "b".to_string())), "新增=绿/删除=红");
    s
}

/// 全域自检汇总（W1 出口评审入口）。
pub fn run_ca_checks() -> Vec<CheckSet> {
    vec![run_infra_checks(), run_cont_checks(), run_logic_checks()]
}

/// AI-03 W2 域自检汇总（#151~#250：名词提取/通俗翻译/简化操作/树根分级）。
pub fn run_ai03_checks() -> Vec<CheckSet> {
    vec![
        nouns::run_nouns_checks(),
        translate::run_translate_checks(),
        simple::run_simple_checks(),
        roottree::run_roottree_checks(),
    ]
}

/// AI-04 W2 域自检汇总（#251~#336：画布/流程图/三界面）。
pub fn run_w2_checks() -> Vec<CheckSet> {
    vec![canvas::run_canvas_checks(), flowchart::run_flowchart_checks(), iface::run_iface_checks()]
}

/// AI-02 W2 域自检汇总（#065~#150：静态分析/动态分析/改进重构/语言识别）。
pub fn run_ai02_checks() -> Vec<CheckSet> {
    vec![
        statics::run_statics_checks(),
        dyna::run_dyna_checks(),
        refactor::run_refactor_checks(),
        langid::run_langid_checks(),
    ]
}

/// 全量自检（W1+W2，共 150 项）。
pub fn run_all_checks() -> Vec<CheckSet> {
    let mut v = run_ca_checks();
    v.extend(run_ai02_checks());
    v.extend(run_w2_checks());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_64_checks_pass() {
        let sets = run_ca_checks();
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 64);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }

    #[test]
    fn ir_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("ca-w1-{}", std::process::id()));
        let proj = tmp.join("proj/src/net.rs");
        std::fs::create_dir_all(proj.parent().unwrap()).unwrap();
        std::fs::write(&proj, "fn ping() {\n  pong()\n}\n").unwrap();
        let ir = ir::open_project(&tmp.join("proj")).unwrap();
        assert_eq!(ir.file_count, 1);
        assert!(ir.loc >= 3);
        assert!(ir.calls.iter().any(|e| e.to == "pong"));
        std::fs::remove_dir_all(&tmp).ok();
    }
}

/// AI-04 W2 域测试。
#[cfg(test)]
mod w2_tests {
    use super::*;

    #[test]
    fn w2_86_checks_pass() {
        let sets = run_w2_checks();
        assert!(sets.iter().map(|s| s.total()).sum::<usize>() >= 86);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }

    #[test]
    fn ai02_86_checks_pass() {
        let sets = run_ai02_checks();
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 86);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }

    #[test]
    fn all_checks_pass() {
        let sets = run_all_checks();
        assert!(sets.iter().map(|s| s.total()).sum::<usize>() >= 236);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }
}
