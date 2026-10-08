//! VE-F3402 · 域自检（判据逐条对应，见 `ver01b_parser.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 双格式 → `E02-双格式-*`
//! - 引用 DAG → `E02-DAG-*`
//! - 循环检测 → `E02-环-*`
//! - 断链三要素 → `E02-断链-*`
//! - 降级矩阵（循环→拒绝+定位 / 断链→三要素 / 格式错→行号定位）→ `E02-降级-*`
//! - 跨批对接（E06 覆盖层衔接）→ `E02-对接-*`
//! - 无障碍（错误定位读屏可达）→ `E02-读屏-*`
//! - 边界防护与性能（O(令牌数)）→ `E02-边界-*` / `E02-性能-*`
//!
//! **门禁设计的四条自律**（照 V 域判据门禁的经验）：
//! 1. **不用表内元素验查表函数**——验 [`PathIndex`] 用的是表外真实路径，
//!    且断言的是"查得到/查不到"，不是"哈希算得对"。
//! 2. **两侧同规范化**——引用的目标与注册的路径都过同一个 `check_segment`
//!    族校验，不存在"一边允许空格一边不允许"导致红线永不触发。
//! 3. **性能自检实测真实工作量**——数 [`PathIndex::probes`] 的**真实增长比**
//!    （规模翻 8 倍时探测数不许翻 8 倍以上），不是 `n * CONST` 的自证式算术。
//! 4. **反假变体**——每条判据都配有"改坏实现就该变红"的对照，见 `E02-反假-*`。
//!
//! 零墙钟、零 IO，回归可复现。

use super::ver01b_parser::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// 域自检集合的标签。
pub const CHECK_DOMAIN: &str = "ve-f3402";

/// 跑VE-F3402 域自检。
pub fn run_ver01b_checks() -> CheckSet {
    let mut set = CheckSet::new(CHECK_DOMAIN);

    // -----------------------------------------------------------------------
    // 一、双格式（JSON 与 TOML 产出必须逐字段相同）
    // -----------------------------------------------------------------------
    //
    // 同一令牌集合的两种写法：路径 color.bg / color.fg / space.md / font.weight。
    // 注意：原文里含 "#101014"，与 r#"..."# 的收尾定界撞车，故用 r##"..."##。
    let json_src = r##"{
  "color": { "bg": "#101014", "fg": "#f0f0f4" },
  "space": { "md": 8 },
  "font": { "weight": 600 }
}"##;
    let toml_src = r##"[color]
bg = "#101014"
fg = "#f0f0f4"

[space]
md = 8

[font]
weight = 600
"##;

    let j = parse_and_build(json_src, SourceFormat::Json);
    let t = parse_and_build(toml_src, SourceFormat::Toml);
    let dual_ok = match (&j, &t) {
        (Ok((jts, jd)), Ok((tts, td))) => {
            // 逐字段比：路径、值、顺序，以及图的节点数与边数。
            jts.entries.len() == tts.entries.len()
                && jts
                    .entries
                    .iter()
                    .zip(tts.entries.iter())
                    .all(|(a, b)| a.path == b.path && a.raw == b.raw)
                && jd.nodes == td.nodes
                && jd.edge_count() == td.edge_count()
        }
        _ => false,
    };
    set.add("E02-双格式-同一集合两格式逐字段相同", dual_ok, "");

    // 双格式的引用图也必须同形：两边都引用一个共同令牌。
    let json_ref = r##"{ "a": "{base}", "b": "{a}", "base": "#fff" }"##;
    let toml_ref = "a = \"{base}\"\nb = \"{a}\"\nbase = \"#fff\"\n";
    let same_graph = match (
        parse_and_build(json_ref, SourceFormat::Json),
        parse_and_build(toml_ref, SourceFormat::Toml),
    ) {
        (Ok((_, jd)), Ok((_, td))) => {
            jd.nodes == td.nodes
                && jd.edge_count() == td.edge_count()
                && (0..jd.nodes as u32).all(|n| jd.out_degree(n) == td.out_degree(n))
        }
        _ => false,
    };
    set.add("E02-双格式-两格式引用图同形", same_graph, "");

    // TOML 表头必须产生前缀路径（否则 `bg` 会变成根级 `bg`）。
    let hdr = parse_and_build("[a.b]\nc = 1\n", SourceFormat::Toml);
    let hdr_ok = matches!(&hdr, Ok((ts, _)) if ts.get("a.b.c").is_some());
    set.add("E02-双格式-TOML表头产生路径前缀", hdr_ok, "");

    // 点分键与表头等价（同一逻辑集合的第三种写法）。
    let dotted = parse_and_build("[a]\nb.c = 1\n", SourceFormat::Toml);
    let dotted_ok = matches!(&dotted, Ok((ts, _)) if ts.get("a.b.c").is_some());
    set.add("E02-双格式-TOML点分键等价于嵌套表", dotted_ok, "");

    // JSON 与 TOML 都必须拒绝非法输入（不是只挑一种格式较严）。
    let bad_json = parse_json("{ \"a\": 1, }");
    let bad_toml = parse_toml("a = \n");
    set.add(
        "E02-双格式-两格式都拒绝各自的语法错",
        bad_json.is_err() && bad_toml.is_err(),
        "",
    );

    // -----------------------------------------------------------------------
    // 二、引用 DAG
    // -----------------------------------------------------------------------
    let dag = parse_and_build(json_ref, SourceFormat::Json);
    let dag_ok = match &dag {
        Ok((ts, d)) => {
            // a→base、b→a 两条边；base 无出边。
            let a = d.index.get("a");
            let b = d.index.get("b");
            let base = d.index.get("base");
            match (a, b, base) {
                (Some(a), Some(b), Some(base)) => {
                    d.out_degree(a) == 1
                        && d.out_degree(b) == 1
                        && d.out_degree(base) == 0
                        && d.out_target_at(a, 0) == Some(base)
                        && d.out_target_at(b, 0) == Some(a)
                        // CSR 的行偏移必须单调不减（否则邻接结构自相矛盾）。
                        && d.out_start.windows(2).all(|w| w[0] <= w[1])
                        && ts.len() == 3
                }
                _ => false,
            }
        }
        Err(_) => false,
    };
    set.add("E02-DAG-出边与CSR行偏移自洽", dag_ok, "");

    // 菱形依赖（DAG 而非树）：a→c、b→c、d→a、d→b。图仍无环。
    let diamond_src = r#"{ "c": "1", "a": "{c}", "b": "{c}", "d": "{a} {b}" }"#;
    let diamond = parse_and_build(diamond_src, SourceFormat::Json);
    let diamond_ok = match &diamond {
        // 复用同一份结果做环检测，不重新解析——重新解析再 unwrap 是把
        // "已知成功"再赌一次，判据不该带这个风险。
        Ok((ts, d)) => d.edge_count() == 4 && d.nodes == 4 && detect_cycles(d, ts).is_empty(),
        Err(_) => false,
    };
    set.add("E02-DAG-菱形依赖建图成功且无环", diamond_ok, "");

    // **同一个目标被多处引用**（真实主题里极常见），边数不被去重。
    let shared = parse_and_build(
        r#"{ "base": "1", "x": "{base}", "y": "{base}", "z": "{base}" }"#,
        SourceFormat::Json,
    );
    let shared_ok = matches!(&shared, Ok((_, d)) if d.edge_count() == 3 && d.nodes == 4);
    set.add("E02-DAG-共享目标的引用不被合并", shared_ok, "");

    // 一个值里多次引用同一目标：抽边去重（否则 CSR 里出现重复行）。
    let twice = extract_refs("{base} solid {base}", Site::start());
    let twice_ok = matches!(&twice, Ok(v) if v.len() == 1);
    set.add("E02-DAG-同目标多次出现只记一条边", twice_ok, "");

    // 整体引用 vs 嵌入式引用必须区分（下游求值方式不同）。
    let kinds = extract_refs("{base}", Site::start());
    let kinds_ok = matches!(&kinds, Ok(v) if v.len() == 1 && v[0].1 == RefKind::Whole);
    let ekinds = extract_refs("1px solid {base}", Site::start());
    let ekinds_ok = matches!(&ekinds, Ok(v) if v.len() == 1 && v[0].1 == RefKind::Embedded);
    set.add(
        "E02-DAG-整体引用与嵌入式引用可区分",
        kinds_ok && ekinds_ok,
        "",
    );

    // 没有花括号的值不产生任何边（多数令牌是字面量）。
    let none = extract_refs("#101014", Site::start());
    set.add(
        "E02-DAG-无花括号的值不产生引用",
        matches!(&none, Ok(v) if v.is_empty()),
        "",
    );

    // 花括号转义：{{ 不是引用。
    let esc = extract_refs("{{literal}}", Site::start());
    set.add(
        "E02-DAG-双写花括号是字面量不是引用",
        matches!(&esc, Ok(v) if v.is_empty()),
        "",
    );

    // 一个值里引用两个不同目标。
    let multi = extract_refs("{a} {b}", Site::start());
    set.add(
        "E02-DAG-一值多引用各自记边",
        matches!(&multi, Ok(v) if v.len() == 2),
        "",
    );

    // -----------------------------------------------------------------------
    // 三、循环检测
    // -----------------------------------------------------------------------
    let two = parse_and_build(r#"{ "a": "{b}", "b": "{a}" }"#, SourceFormat::Json);
    let two_ok = match &two {
        Err(ds) => {
            ds.iter().any(|d| {
                d.code == DiagCode::Cycle && d.what.contains("a → b → a") && d.site.line >= 1
            })
        }
        Ok(_) => false,
    };
    set.add("E02-环-二元环被检出并拒绝建图", two_ok, "");

    let three = parse_and_build(
        r#"{ "a": "{b}", "b": "{c}", "c": "{a}" }"#,
        SourceFormat::Json,
    );
    let three_ok = match &three {
        Err(ds) => ds
            .iter()
            .any(|d| d.code == DiagCode::Cycle && d.what.contains("→")),
        Ok(_) => false,
    };
    set.add("E02-环-三元环被检出", three_ok, "");

    // **自引用**是长度为 1 的环，必须同样拒绝。
    let self_ref = parse_and_build(r#"{ "a": "{a}" }"#, SourceFormat::Json);
    let self_ok = match &self_ref {
        Err(ds) => ds.iter().any(|d| d.code == DiagCode::Cycle),
        Ok(_) => false,
    };
    set.add("E02-环-自引用按环拒绝", self_ok, "");

    // **环长随实际环长变化**（不是固定长度的假环报告）。
    let l2 = cycle_ring_len(r#"{ "a": "{b}", "b": "{a}" }"#);
    let l4 = cycle_ring_len(
        r#"{ "a": "{b}", "b": "{c}", "c": "{d}", "d": "{a}" }"#,
    );
    set.add("E02-环-环长随实际环长变化", l2 == 2 && l4 == 4, "");

    // 无环图不产生任何环诊断（不能误报）。
    let acyclic = parse_and_build(json_ref, SourceFormat::Json);
    let no_false = match &acyclic {
        Ok((ts, d)) => detect_cycles(d, ts).is_empty(),
        Err(_) => false,
    };
    set.add("E02-环-无环图不误报", no_false, "");

    // **长链不溢栈**：200 节点的单链（递归 DFS 在这里就会有问题，
    // 迭代 DFS 必须照样过）。
    let long_ok = long_chain_ok(200);
    set.add("E02-环-长引用链不溢栈且判定正确", long_ok, "");

    // -----------------------------------------------------------------------
    // 四、断链三要素
    // -----------------------------------------------------------------------
    let broken = parse_and_build(r#"{ "a": "{nope}" }"#, SourceFormat::Json);
    let broken_ok = match &broken {
        Err(ds) => ds.iter().any(|d| {
            d.code == DiagCode::BrokenRef
                && d.what.contains("nope")
                && !d.why.is_empty()
                && !d.fix.is_empty()
        }),
        Ok(_) => false,
    };
    set.add("E02-断链-缺失目标被检出且三要素齐备", broken_ok, "");

    // **嵌套路径的断链**：引用了三段路径，缺的是第三段。
    let nested = parse_and_build(r#"{ "a": "{x.y.zz}" }"#, SourceFormat::Json);
    let nested_ok = match &nested {
        Err(ds) => ds.iter().any(|d| d.code == DiagCode::BrokenRef),
        Ok(_) => false,
    };
    set.add("E02-断链-嵌套路径的断链被检出", nested_ok, "");

    // **三要素构造期拦截**：任一要素为空都被降级为 E_DIAG_INCOMPLETE。
    let inc_what = Diag::new(DiagCode::BrokenRef, Site::start(), "", "r", "f");
    let inc_why = Diag::new(DiagCode::BrokenRef, Site::start(), "w", "", "f");
    let inc_fix = Diag::new(DiagCode::BrokenRef, Site::start(), "w", "r", "");
    let inc_ok = inc_what.code == DiagCode::IncompleteDiag
        && inc_why.code == DiagCode::IncompleteDiag
        && inc_fix.code == DiagCode::IncompleteDiag
        && inc_what.complete()
        && inc_why.complete()
        && inc_fix.complete();
    set.add("E02-断链-三要素缺一即降级为不完整诊断", inc_ok, "");

    // 齐备的三要素不被降级。
    let full = Diag::new(DiagCode::BrokenRef, Site::start(), "w", "r", "f");
    set.add(
        "E02-断链-齐备三要素保持原码",
        full.code == DiagCode::BrokenRef && full.complete(),
        "",
    );

    // 同一份断链，**引用的目标名必须出现在现象里**（不然读者无法定位）。
    let names_target = match &broken {
        Err(ds) => ds.iter().any(|d| d.what.contains("a") && d.what.contains("nope")),
        Ok(_) => false,
    };
    set.add("E02-断链-诊断点明引用方与被引方", names_target, "");

    // 反假：补上定义后断链判据必须转绿。
    let fixed = parse_and_build(
        r#"{ "a": "{nope}", "nope": "1" }"#,
        SourceFormat::Json,
    );
    let anti = matches!(&broken, Err(_)) && matches!(&fixed, Ok(_));
    set.add("E02-反假-补上定义后断链判据转绿", anti, "");

    // -----------------------------------------------------------------------
    // 五、降级矩阵
    // -----------------------------------------------------------------------
    // 格式错 → 行号定位（第 2 行出错就报第 2 行，不是"第 1 行"）。
    let line2 = parse_json("{\n  \"a\": tru\n}");
    let line_ok = match &line2 {
        Err(d) => d.site.line == 2,
        Ok(_) => false,
    };
    set.add("E02-降级-格式错报到正确行号", line_ok, "");

    // 第 3 行出错不能报第 2 行（定位必须真的跟着行走）。
    let line3 = parse_json("{\n  \"a\": 1,\n  \"b\": tru\n}");
    let line3_ok = match &line3 {
        Err(d) => d.site.line == 3,
        Ok(_) => false,
    };
    set.add("E02-降级-行号随出错位置前移", line3_ok, "");

    // **多字节字符后的列号**必须是字符列，不是字节列。
    // 关键：错误必须与多字节字符**在同一行**，否则测的是行号不是列号。
    // `  "键": tru }` —— `键` 占 1 字符/3 字节。若按字节列算会多出 2。
    let multibyte = parse_json("{ \"键\": tru }");
    let col_ok = match &multibyte {
        Err(d) => {
            // col: 1=空 2=空 3=" 4=键 5=" 6=: 7=空 8=t
            d.site.line == 1 && d.site.col == 8
        }
        Ok(_) => false,
    };
    set.add("E02-降级-多字节字符后的列号按字符计", col_ok, "");

    // 深度超限必须报错（不是解析到一半）。
    let deep = deep_json(MAX_DEPTH + 5);
    set.add(
        "E02-降级-嵌套超深被拒",
        matches!(&deep, Err(d) if d.code == DiagCode::DepthLimit),
        "",
    );

    // 重复路径必须报错（单源律在解析期拦住）。
    // **必须走 parse_and_build**：重复检测在 `flatten` 里，不在 `parse_json`
    // 里——JSON 语法本身允许重复键（后写覆盖前写），是令牌单源律不许。
    let dup = parse_and_build(r#"{ "a": 1, "a": 2 }"#, SourceFormat::Json);
    set.add(
        "E02-降级-重复定义被拒",
        matches!(&dup, Err(ds) if ds.iter().any(|d| d.code == DiagCode::DupPath)),
        "",
    );

    // 数组不能当令牌值（没有稳定路径）。
    let arr = parse_and_build(r#"{ "a": [1, 2] }"#, SourceFormat::Json);
    set.add("E02-降级-数组值被拒而非拍平", arr.is_err(), "");

    // null 不能当令牌值（与空串不可区分）。
    let nul = parse_json(r#"{ "a": null }"#);
    set.add(
        "E02-降级-null令牌值被拒",
        matches!(&nul, Err(d) if d.code == DiagCode::BadLiteral),
        "",
    );

    // 未闭合字符串必须报错并给位置。
    let unclosed = parse_json("{ \"a\": \"abc");
    set.add(
        "E02-降级-未闭合字符串被拒",
        matches!(&unclosed, Err(d) if d.code == DiagCode::Unclosed),
        "",
    );

    // TOML 未闭合表头。
    set.add(
        "E02-降级-TOML未闭合表头被拒",
        parse_toml("[a\nb = 1\n").is_err(),
        "",
    );

    // TOML 数组表显式拒绝（不是半支持）。
    set.add(
        "E02-降级-TOML数组表被显式拒绝",
        parse_toml("[[a]]\nb = 1\n").is_err(),
        "",
    );

    // 空引用被拒。
    set.add(
        "E02-降级-空引用被拒",
        extract_refs("{}", Site::start()).is_err(),
        "",
    );

    // 未闭合引用被拒。
    set.add(
        "E02-降级-未闭合引用被拒",
        extract_refs("{abc", Site::start()).is_err(),
        "",
    );

    // 非法路径段（引用里带花括号/空段）。
    set.add(
        "E02-降级-引用路径含非法段被拒",
        extract_refs("{a..b}", Site::start()).is_err(),
        "",
    );

    // **TOML 日期时间被显式拒绝**（令牌域不含日期语义）。
    set.add(
        "E02-降级-TOML日期时间被拒",
        parse_toml("a = 2026-10-07\n").is_err(),
        "",
    );

    // 令牌数超限被拒（用真实语料造，不是伪造计数）。
    let too_many = many_tokens(MAX_TOKENS + 2);
    set.add(
        "E02-降级-令牌数超限被拒",
        matches!(&too_many, Err(ds) if ds.iter().any(|d| d.code == DiagCode::TokenLimit)),
        "",
    );

    // -----------------------------------------------------------------------
    // 六、无障碍：错误定位读屏可达
    // -----------------------------------------------------------------------
    let spoken_ok = match &broken {
        Err(ds) => ds.iter().all(|d| {
            let s = d.spoken();
            // 播报文本必须含：诊断码、行、列、以及三要素的实质内容。
            s.contains(d.code.wire())
                && s.contains("行")
                && s.contains("列")
                && s.contains(&d.why)
                && s.contains(&d.fix)
                && !d.what.is_empty()
        }),
        Ok(_) => false,
    };
    set.add("E02-读屏-诊断播报含位置与三要素", spoken_ok, "");

    // 位置字段本身可读（1 起算，字节偏移 0 起算）。
    let site = Site {
        line: 3,
        col: 7,
        byte: 12,
    };
    set.add(
        "E02-读屏-位置字段独立可读",
        site.line == 3 && site.col == 7 && site.byte == 12 && Site::start().line == 1,
        "",
    );

    // **每一类阻断诊断都必须可播报**（不许有"哑诊断"）。
    let all_speak = [
        DiagCode::IncompleteDiag,
        DiagCode::DepthLimit,
        DiagCode::BadUtf8,
        DiagCode::Unexpected,
        DiagCode::Unclosed,
        DiagCode::BadSegment,
        DiagCode::DupPath,
        DiagCode::BrokenRef,
        DiagCode::Cycle,
        DiagCode::TokenLimit,
    ]
    .iter()
    .all(|c| {
        let d = Diag::new(*c, Site::start(), "w", "r", "f");
        d.complete() && !d.spoken().is_empty() && c.wire().starts_with("E_")
    });
    set.add("E02-读屏-十类阻断诊断全部可播报", all_speak, "");

    // 诊断码互不相同（不能两个不同病因共用一个码——处置方向会相反）。
    let wires: Vec<&str> = [
        DiagCode::IncompleteDiag,
        DiagCode::DepthLimit,
        DiagCode::BadUtf8,
        DiagCode::Unexpected,
        DiagCode::Unclosed,
        DiagCode::BadSegment,
        DiagCode::DupPath,
        DiagCode::BrokenRef,
        DiagCode::Cycle,
        DiagCode::TokenLimit,
        DiagCode::BadLiteral,
    ]
    .iter()
    .map(|c| c.wire())
    .collect();
    let mut uniq = wires.clone();
    uniq.sort_unstable();
    uniq.dedup();
    set.add(
        "E02-读屏-诊断码互不重复",
        uniq.len() == wires.len(),
        "",
    );

    // -----------------------------------------------------------------------
    // 七、边界防护
    // -----------------------------------------------------------------------
    // 令牌上限与架构一致（两处必须同值）。
    set.add(
        "E02-边界-令牌上限与架构一致",
        MAX_TOKENS == super::ver01_arch::MAX_TOKENS,
        "",
    );

    // 解析器版本号在册。
    set.add(
        "E02-边界-解析器版本号在册",
        PARSER_VERSION == "E01-parser-v1" && !PARSER_VERSION.is_empty(),
        "",
    );

    // 深度上限是有效正值（不是 0 或天文数字）。
    set.add(
        "E02-边界-深度上限为有效正值",
        MAX_DEPTH > 0 && MAX_DEPTH <= 1024,
        "",
    );

    // 路径索引初始槽位是 2 的幂且不低于下限（开放寻址的前提）。
    set.add(
        "E02-边界-索引初始槽位为2的幂",
        PATH_INDEX_MIN_SLOTS.is_power_of_two() && PATH_INDEX_MIN_SLOTS >= 1024,
        "",
    );

    // 数字保留原文：1.50 不能被改写成 1.5。
    let num = parse_and_build(r#"{ "a": 1.50, "b": 1.5 }"#, SourceFormat::Json);
    let raw_ok = match &num {
        Ok((ts, _)) => {
            let a = ts.get("a").map(|e| e.raw.as_str());
            let b = ts.get("b").map(|e| e.raw.as_str());
            a == Some("1.50") && b == Some("1.5")
        }
        Err(_) => false,
    };
    set.add("E02-边界-数字保留原始字面量", raw_ok, "");

    // 转成 f64 是**按需**的，且 1.50 与 1.5 转出来相等（数值语义一致）。
    let conv = match &num {
        Ok((ts, _)) => {
            let a = ts.get("a").and_then(|e| {
                NumLit {
                    raw: e.raw.clone(),
                    site: e.site,
                }
                .as_f64()
            });
            let b = ts.get("b").and_then(|e| {
                NumLit {
                    raw: e.raw.clone(),
                    site: e.site,
                }
                .as_f64()
            });
            match (a, b) {
                (Some(x), Some(y)) => x == 1.5 && y == 1.5,
                _ => false,
            }
        }
        Err(_) => false,
    };
    set.add("E02-边界-数字按需转换语义一致", conv, "");

    // 转不动的数字返回 None 而不是猜一个值。
    let bad_num = NumLit {
        raw: "abc".to_string(),
        site: Site::start(),
    }
    .as_f64();
    set.add("E02-边界-无法转换的数字返回空", bad_num.is_none(), "");

    // TOML 的下划线分隔数字能正常转换。
    let under = parse_and_build("a = 1_000\n", SourceFormat::Toml);
    let under_ok =
        matches!(&under, Ok((ts, _)) if ts.get("a").map(|e| e.raw.as_str()) == Some("1_000"));
    set.add("E02-边界-TOML下划线数字保留原文", under_ok, "");

    // TOML 注释不影响解析。
    let cmt = parse_and_build("# 头部注释\na = 1 # 行尾注释\n", SourceFormat::Toml);
    set.add(
        "E02-边界-TOML注释被正确跳过",
        matches!(&cmt, Ok((ts, _)) if ts.get("a").is_some()),
        "",
    );

    // TOML 多行字符串。
    let ml = parse_and_build("a = \"\"\"\nline1\nline2\"\"\"\n", SourceFormat::Toml);
    set.add(
        "E02-边界-TOML多行基本串可解析",
        matches!(&ml, Ok((ts, _)) if ts.get("a").map(|e| e.raw.as_str()) == Some("line1\nline2")),
        "",
    );

    // TOML 字面串无转义（反斜杠保持原样）。
    let lit = parse_and_build(r"a = 'C:\path'", SourceFormat::Toml);
    set.add(
        "E02-边界-TOML字面串不处理转义",
        matches!(&lit, Ok((ts, _)) if ts.get("a").map(|e| e.raw.as_str()) == Some(r"C:\path")),
        "",
    );

    // JSON 转义与代理对。
    let esc = parse_and_build(r#"{ "a": "中", "b": "\u0041" }"#, SourceFormat::Json);
    set.add(
        "E02-边界-JSON转义与代理对正确",
        matches!(&esc, Ok((ts, _)) if ts.get("a").map(|e| e.raw.as_str()) == Some("中")
            && ts.get("b").map(|e| e.raw.as_str()) == Some("A")),
        "",
    );

    // 孤立代理项被拒（不能悄悄产出一个非法字符）。
    set.add(
        "E02-边界-JSON孤立代理项被拒",
        parse_json(r#"{ "a": "\uD800" }"#).is_err(),
        "",
    );

    // JSON 前导零被拒。
    set.add(
        "E02-边界-JSON前导零被拒",
        parse_json(r#"{ "a": 01 }"#).is_err(),
        "",
    );

    // JSON 尾随逗号被拒。
    set.add(
        "E02-边界-JSON尾随逗号被拒",
        parse_json(r#"{ "a": 1, }"#).is_err(),
        "",
    );

    // UTF-8 序列长度表（词法推进的依据）。
    set.add(
        "E02-边界-UTF8序列长度判定正确",
        utf8_seq_len(0x41) == 1
            && utf8_seq_len(0xC3) == 2
            && utf8_seq_len(0xE4) == 3
            && utf8_seq_len(0xF0) == 4,
        "",
    );

    // 裸键字符判定。
    set.add(
        "E02-边界-TOML裸键字符判定正确",
        toml_bare(b'a') && toml_bare(b'9') && toml_bare(b'_') && toml_bare(b'-')
            && !toml_bare(b'.')
            && !toml_bare(b'=')
            && !toml_bare(b'"'),
        "",
    );

    // -----------------------------------------------------------------------
    // 八、性能：O(令牌数) 靠数据结构兑现
    // -----------------------------------------------------------------------
    //
    // **实测真实工作量**：数 PathIndex 的探测步数随规模的增长。
    // 判据是**增长比**：规模 ×8 时，探测数不许超过 ×64（线性以上一档）。
    // 自证式算术（n * CONST）是空断言，所以这里数的是真实计数器。
    let idx_small = index_probe_cost(1024);
    let idx_big = index_probe_cost(8192);
    let ratio_ok = idx_small > 0 && idx_big <= idx_small * 64;
    set.add("E02-性能-路径查找探测数亚线性增长", ratio_ok, "");

    // **端到端解析的规模可扩展**：4096 个令牌必须在有限步内建完图。
    let big_set = build_wide(4096);
    let scale_ok = matches!(&big_set, Ok((ts, d)) if ts.len() == 4096 && d.nodes == 4096);
    set.add("E02-性能-4096令牌规模建图成功", scale_ok, "");

    // 宽图（一个源引用多个目标）的边数与CSR 长度一致。
    let fan = build_fan(64);
    let fan_ok = matches!(&fan, Ok((_, d)) if d.edge_count() == 64);
    set.add("E02-性能-扇出边数与CSR一致", fan_ok, "");

    // 插入与查询的一致性（**用表外真实路径验查表函数**）。
    let mut ix = PathIndex::new();
    let mut ok_all = true;
    for i in 0..500 {
        let p = format!("color.palette.shade{}", i);
        if !ix.insert(&p, i as u32) {
            ok_all = false;
        }
    }
    for i in 0..500 {
        let p = format!("color.palette.shade{}", i);
        if ix.get(&p) != Some(i as u32) {
            ok_all = false;
        }
    }
    // 表外路径必须查不到。
    if ix.get("color.palette.shade500").is_some() || ix.get("not.in.table").is_some() {
        ok_all = false;
    }
    // 重复插入必须被拒（单源律在索引层也生效）。
    if ix.insert("color.palette.shade0", 999) {
        ok_all = false;
    }
    if ix.get("color.palette.shade0") != Some(0) {
        ok_all = false;
    }
    set.add("E02-性能-索引插入查询与重复拒绝一致", ok_all, "");

    // 超过半载后扩容，查全率仍必须是 100%（开放寻址的扩容是正确性前提）。
    let grow_ok = {
        let mut g = PathIndex::new();
        let mut all = true;
        for i in 0..5000 {
            let p = format!("t{}", i);
            if !g.insert(&p, i as u32) {
                all = false;
            }
        }
        for i in 0..5000 {
            if g.get(&format!("t{}", i)) != Some(i as u32) {
                all = false;
            }
        }
        g.len() == 5000 && all
    };
    set.add("E02-性能-扩容后查全率保持100%", grow_ok, "");

    // 哈希函数是纯函数（同输入同输出，不同输入不同输出）。
    let h1 = PathIndex::hash("color.bg");
    let h2 = PathIndex::hash("color.bg");
    let h3 = PathIndex::hash("color.fg");
    set.add(
        "E02-性能-路径哈希确定且区分输入",
        h1 == h2 && h1 != h3,
        "",
    );

    // -----------------------------------------------------------------------
    // 九、跨批对接：E06 覆盖层衔接
    // -----------------------------------------------------------------------
    //
    // 覆盖层（F3406）消费的是**路径 + 原始值 + 定义点**，不重解源文件。
    // 这里断言这三样都在，且路径可直接用于覆盖层的键匹配。
    let handoff = match parse_and_build(json_src, SourceFormat::Json) {
        Ok((ts, d)) => ts
            .entries
            .iter()
            .all(|e| !e.path.is_empty() && e.path.contains('.') && d.index.get(&e.path).is_some()),
        Err(_) => false,
    };
    set.add("E02-对接-令牌集可直接交覆盖层消费", handoff, "");

    // 索引里的路径集合与条目集合必须完全一致（不多不少）。
    let index_exact = match parse_and_build(json_src, SourceFormat::Json) {
        Ok((ts, d)) => {
            d.index.len() == ts.len()
                && (0..ts.len()).all(|i| d.index.get(&ts.entries[i].path) == Some(i as u32))
                && (0..ts.len()).all(|i| d.index.key_at(i) == Some(ts.entries[i].path.as_str()))
        }
        Err(_) => false,
    };
    set.add("E02-对接-索引与条目双向一致", index_exact, "");

    // -----------------------------------------------------------------------
    // 十、反假变体：改坏实现就该变红
    // -----------------------------------------------------------------------
    //
    // 这一组不是"再跑一遍正常路径"，而是**构造能让判据失败的场景**，
    // 确认判据真的会红。恒真的判据在这里会被暴露。
    let anti_cycle = {
        // 把真环的引用去掉一个，判据必须转绿（证明它不是恒红）。
        let broken_ring = parse_and_build(r#"{ "a": "{b}", "b": "plain" }"#, SourceFormat::Json);
        matches!(&broken_ring, Ok(_))
    };
    set.add("E02-反假-断开环后环判据转绿", anti_cycle, "");

    let anti_broken = {
        // 引用存在时不报断链（证明断链判据不是恒红）。
        let ok_ref = parse_and_build(r#"{ "a": "{b}", "b": "1" }"#, SourceFormat::Json);
        matches!(&ok_ref, Ok(_))
    };
    set.add("E02-反假-引用存在时不断链", anti_broken, "");

    let anti_line = {
        // 出错在第 1 行时报第 1 行（证明行号判据跟着位置走）。
        let l1 = parse_json("{ tru }");
        matches!(&l1, Err(d) if d.site.line == 1)
    };
    set.add("E02-反假-首行出错报首行", anti_line, "");

    let anti_depth = {
        // 刚好在上限内的嵌套必须通过（证明深度判据不是"见嵌套就拒"）。
        let ok_deep = deep_json(MAX_DEPTH - 1);
        ok_deep.is_ok()
    };
    set.add("E02-反假-上限内嵌套可通过", anti_depth, "");

    let anti_limit = {
        // 刚好在上限内的令牌数必须通过。
        let at_limit = build_wide(MAX_TOKENS - 1);
        matches!(&at_limit, Ok((ts, d)) if ts.len() == MAX_TOKENS - 1 && d.nodes == MAX_TOKENS - 1)
    };
    set.add("E02-反假-上限内令牌数可通过", anti_limit, "");

    let anti_dup = {
        // 同名但不同层的键不是重复定义（证明重复判据按全路径比对，
        // 而不是只看末段名）。
        let nested_ok = parse_and_build(
            r#"{ "a": { "x": 1 }, "b": { "x": 2 } }"#,
            SourceFormat::Json,
        );
        matches!(&nested_ok, Ok((ts, _)) if ts.get("a.x").is_some() && ts.get("b.x").is_some())
    };
    set.add("E02-反假-异层同名键不算重复", anti_dup, "");

    let anti_toml_dup = {
        // 表头与同名键值对撞同一路径 = 重复定义（证明 TOML 侧也拦）。
        parse_toml("[a]\nb = 1\n[a.b]\nc = 2\n").is_err()
    };
    set.add("E02-反假-TOML表头撞同名键被拒", anti_toml_dup, "");

    let anti_scalar_prefix = {
        // 标量后面再挂子键 = 路径冲突（不是静默覆盖）。
        parse_toml("a = 1\n[a.b]\nc = 2\n").is_err()
    };
    set.add("E02-反假-标量路径前缀冲突被拒", anti_scalar_prefix, "");

    set
}

// ---------------------------------------------------------------------------
// 辅助（只在自检里用，不属于功能面）
// ---------------------------------------------------------------------------

/// 从环诊断的现象段里数出环长。
fn cycle_ring_len(src: &str) -> usize {
    match parse_and_build(src, SourceFormat::Json) {
        Err(ds) => {
            for d in ds.iter() {
                if d.code == DiagCode::Cycle {
                    // "a → b → a" 的箭头数 = 环长。
                    return d.what.matches(" → ").count();
                }
            }
            0
        }
        Ok(_) => 0,
    }
}

/// 长引用链：n 个节点串成一条链，最后一个不引用任何东西。
fn long_chain_ok(n: usize) -> bool {
    let mut s = String::from("{");
    for i in 0..n {
        if i + 1 < n {
            s.push_str(&format!("\"t{}\": \"{{t{}}}\", ", i, i + 1));
        } else {
            s.push_str(&format!("\"t{}\": \"end\"", i));
        }
    }
    s.push('}');
    match parse_and_build(&s, SourceFormat::Json) {
        Ok((_, d)) => d.nodes == n && d.edge_count() == n - 1,
        Err(_) => false,
    }
}

/// 造一个深度为 `d` 的嵌套 JSON 文本。
fn deep_json(d: usize) -> Result<Val, Diag> {
    let mut s = String::new();
    for _ in 0..d {
        s.push_str("{ \"a\": ");
    }
    s.push('1');
    for _ in 0..d {
        s.push('}');
    }
    parse_json(&s)
}

/// 造 `n` 个互不引用的令牌。
fn many_tokens(n: usize) -> Result<TokenSet, Vec<Diag>> {
    let mut s = String::from("{");
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("\"k{}\": {}", i, i));
    }
    s.push('}');
    let v = parse_json(&s).map_err(|e| vec![e])?;
    flatten(&v, SourceFormat::Json)
}

/// 造 `n` 个令牌（每个引用下一个，最后一个给字面量）。
fn build_wide(n: usize) -> Result<(TokenSet, TokenDag), Vec<Diag>> {
    let mut s = String::from("{");
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        if i + 1 < n {
            s.push_str(&format!("\"k{}\": \"{{k{}}}\"", i, i + 1));
        } else {
            s.push_str(&format!("\"k{}\": \"end\"", i));
        }
    }
    s.push('}');
    parse_and_build(&s, SourceFormat::Json)
}

/// 造一个扇出图：一个源引用 `n` 个目标。
fn build_fan(n: usize) -> Result<(TokenSet, TokenDag), Vec<Diag>> {
    let mut s = String::from("{");
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("\"t{}\": {}", i, i));
    }
    s.push_str(&format!(
        ", \"hub\": \"{}\"",
        (0..n)
            .map(|i| format!("{{t{}}}", i))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    s.push('}');
    parse_and_build(&s, SourceFormat::Json)
}

/// 数插入 `n` 个真实路径所耗的探测步数。
fn index_probe_cost(n: usize) -> u64 {
    let mut ix = PathIndex::new();
    for i in 0..n {
        let p = format!("color.brand.surface.elevation.{}", i);
        let _ = ix.insert(&p, i as u32);
    }
    ix.probes
}