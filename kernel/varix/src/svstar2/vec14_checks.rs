//! VE-F0414 · 域自检（判据逐条对应，见 `vec14_include.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 搜索序显性 → `C14-序-*`（引号先查包含者目录、尖括号不查、引号目录优先于
//!   引号搜索路径、绝对路径直试、搜索序明细非空、找到即停不试后续目录）
//! - 环检测输出环 → `C14-环-*`（直接自环、两节点环、三节点环、环路径全文、
//!   环上指纹序列、深链不成环不误报）
//! - 包含图 → `C14-图-*`（节点登记、边登记、邻接表、无环性、重复边不重复计数）
//! - 缓存裁定 → `C14-缓-*`（Once 跳过、跳过出注记、Repeat 重展开、指纹变则重
//!   展开并标注、缓存条目登记、命中 O(1) 计数）
//! - 深度上限 → `C14-深-*`（超限报错、报错指向链顶、链快照非空）
//! - 零静默 → `C14-显性-*`（三要素齐备、找不到带明细、错误码不合并）

// no_std 下 std prelude 不存在：`String` 与 `format!`/`vec!` 都得显式引入。
// 宿主 `cargo test` 有std prelude 会掩盖这一点，整树 `cargo check --lib`
// 才暴露——两处都写上，两条链路都成立。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;

use super::vec14_include::*;
use crate::checks::CheckSet;

/// 组装一个三文件测试源：`main.vec` 引 `a.h`，`a.h` 引 `b.h`，`b.h` 无包含。
fn chain_source() -> MemorySource {
    let s = MemorySource::new();
    s.insert("main.vec", "#include \"a.h\"\nvoid main() {}\n");
    s.insert("a.h", "#include \"b.h\"\nA_BODY\n");
    s.insert("b.h", "B_BODY\n");
    s
}

/// 解析并取展开文本（成功 Ok 文本，失败 Ok 错误码——判据断言用）。
fn run(src: &MemorySource, cfg: &IncludeConfig, root: &str) -> Result<String, &'static str> {
    match resolve_includes(src, cfg, root, src.get(root).unwrap_or_default().as_str()) {
        Ok((text, _, _, _)) => Ok(text),
        Err(e) => Err(e.code),
    }
}

/// 深链解析（自引用 a→b→c），用于深度上限判据。
fn deep_source(depth: usize) -> MemorySource {
    let s = MemorySource::new();
    s.insert("m.vec", "#include \"f0.h\"\n");
    for i in 0..depth {
        s.insert(
            &format!("f{}.h", i),
            &format!("#include \"f{}.h\"\n", i + 1),
        );
    }
    s.insert(&format!("f{}.h", depth), "LEAF\n");
    s
}

/// VE-F0414 域自检。
pub fn run_vec14_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec14");

    // ---- 判据：搜索序显性 ----

    // 引号形式先查包含者所在目录：a.h 与 inc/a.h 同名时，命中包含者目录那份
    {
        let s = MemorySource::new();
        s.insert("sub/m.vec", "#include \"a.h\"\n");
        s.insert("sub/a.h", "NEAR\n");
        s.insert("a.h", "ROOT\n");
        let cfg = IncludeConfig::new(16);
        let r = run(&s, &cfg, "sub/m.vec");
        set.add(
            "C14-序-引号先查包含者目录",
            r.as_deref() == Ok("NEAR\n"),
            "",
        );
    }
    // 尖括号形式**不查**包含者所在目录：只看尖括号搜索路径（判据一的语义核心）
    {
        let s = MemorySource::new();
        s.insert("sub/m.vec", "#include <a.h>\n");
        s.insert("sub/a.h", "NEAR\n");
        s.insert("lib/a.h", "LIB\n");
        let cfg = IncludeConfig::new(16).with_angle_dir("lib");
        let r = run(&s, &cfg, "sub/m.vec");
        set.add(
            "C14-序-尖括号不查包含者目录",
            r.as_deref() == Ok("LIB\n"),
            "尖括号若也查包含者目录，同名头就会被私有文件劫持",
        );
    }
    // 引号搜索路径在包含者目录之后（顺序显性：先目录后路径）
    {
        let s = MemorySource::new();
        s.insert("sub/m.vec", "#include \"a.h\"\n");
        s.insert("sub/a.h", "NEAR\n");
        s.insert("q/a.h", "QUOTE\n");
        let cfg = IncludeConfig::new(16).with_quote_dir("q");
        let r = run(&s, &cfg, "sub/m.vec");
        set.add(
            "C14-序-包含者目录优先于搜索路径",
            r.as_deref() == Ok("NEAR\n"),
            "",
        );
    }
    // 包含者目录没有时才落到引号搜索路径
    {
        let s = MemorySource::new();
        s.insert("sub/m.vec", "#include \"a.h\"\n");
        s.insert("q/a.h", "QUOTE\n");
        let cfg = IncludeConfig::new(16).with_quote_dir("q");
        let r = run(&s, &cfg, "sub/m.vec");
        set.add(
            "C14-序-目录未命中落到搜索路径",
            r.as_deref() == Ok("QUOTE\n"),
            "",
        );
    }
    // 绝对路径直试（不进搜索序）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"/abs/a.h\"\n");
        s.insert("/abs/a.h", "ABS\n");
        let cfg = IncludeConfig::new(16);
        let r = run(&s, &cfg, "m.vec");
        set.add("C14-序-绝对路径直试", r.as_deref() == Ok("ABS\n"), "");
    }
    // 找到即停：命中后不再试后续目录（探针数守恒，性能判据面）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("d1/a.h", "ONE\n");
        s.insert("d2/a.h", "TWO\n");
        s.insert("d3/a.h", "THREE\n");
        let cfg = IncludeConfig::new(16)
            .with_quote_dir("d1")
            .with_quote_dir("d2")
            .with_quote_dir("d3");
        let (_, _, _, st) = resolve_includes(&s, &cfg, "m.vec", "#include \"a.h\"\n").unwrap();
        // 一次 lookup：命中即停，不把后两个目录也读一遍
        set.add("C14-序-找到即停", st.lookups == 1, "");
    }
    // 找不到时报错**带搜索序明细**（判据一原文要求）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"nope.h\"\n");
        let cfg = IncludeConfig::new(16)
            .with_quote_dir("q1")
            .with_quote_dir("q2");
        match resolve_includes(&s, &cfg, "m.vec", "#include \"nope.h\"\n") {
            Err(e) => set.add(
                "C14-序-找不到带搜索序明细",
                e.code == "E_INC_NOT_FOUND"
                    // 三次探针：包含者所在目录（根级文件 = 当前目录）+两个
                    // 引号搜索路径。少一次就说明根级包含者的目录又被跳过了。
                    && e.search_trace.probes() == 3
                    && e.search_trace.detail().contains("q1/nope.h")
                    && e.search_trace.detail().contains("q2/nope.h"),
                "",
            ),
            Ok(_) => set.add("C14-序-找不到带搜索序明细", false, "本应报错却成功了"),
        }
    }

    // ---- 判据：环检测输出环 ----

    // 直接自环（a 含 a）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "#include \"a.h\"\n");
        let cfg = IncludeConfig::new(16);
        match resolve_includes(&s, &cfg, "m.vec", "#include \"a.h\"\n") {
            Err(e) => set.add(
                "C14-环-直接自环",
                e.code == "E_INC_CYCLE" && e.cycle_path == "a.h → a.h",
                "",
            ),
            Ok(_) => set.add("C14-环-直接自环", false, "本应报环却成功了"),
        }
    }
    // 两节点环（a → b → a）且环路径全文正确（判据二原文要求「输出环路径」）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "#include \"b.h\"\n");
        s.insert("b.h", "#include \"a.h\"\n");
        let cfg = IncludeConfig::new(16);
        match resolve_includes(&s, &cfg, "m.vec", "#include \"a.h\"\n") {
            Err(e) => set.add(
                "C14-环-两节点环路径全文",
                e.code == "E_INC_CYCLE" && e.cycle_path == "a.h → b.h → a.h",
                "",
            ),
            Ok(_) => set.add("C14-环-两节点环路径全文", false, "本应报环却成功了"),
        }
    }
    // 三节点环
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "#include \"b.h\"\n");
        s.insert("b.h", "#include \"c.h\"\n");
        s.insert("c.h", "#include \"a.h\"\n");
        let cfg = IncludeConfig::new(16);
        match resolve_includes(&s, &cfg, "m.vec", "#include \"a.h\"\n") {
            Err(e) => set.add(
                "C14-环-三节点环",
                e.code == "E_INC_CYCLE" && e.cycle_path == "a.h → b.h → c.h → a.h",
                "",
            ),
            Ok(_) => set.add("C14-环-三节点环", false, "本应报环却成功了"),
        }
    }
    // 环上指纹序列（含重复起点段，便于对拍）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "#include \"a.h\"\n");
        let cfg = IncludeConfig::new(16);
        match resolve_includes(&s, &cfg, "m.vec", "#include \"a.h\"\n") {
            Err(e) => set.add(
                "C14-环-环上指纹序列",
                e.cycle_fingerprints.len() == 2
                    && e.cycle_fingerprints[0] == e.cycle_fingerprints[1],
                "",
            ),
            Ok(_) => set.add("C14-环-环上指纹序列", false, "本应报环却成功了"),
        }
    }
    // 深链**不**误报成环（a→b→c 无环，必须正常展开——环检测不许假阳性）
    {
        let s = chain_source();
        let cfg = IncludeConfig::new(16);
        let r = run(&s, &cfg, "main.vec");
        set.add(
            "C14-环-无环不误报",
            r.as_deref() == Ok("B_BODY\nA_BODY\nvoid main() {}\n"),
            "",
        );
    }

    // ---- 判据：包含图 ----

    // 节点与边登记（含根）
    {
        let s = chain_source();
        let cfg = IncludeConfig::new(16);
        let (_, g, _, _) =
            resolve_includes(&s, &cfg, "main.vec", s.get("main.vec").unwrap().as_str()).unwrap();
        set.add(
            "C14-图-节点登记",
            g.nodes.contains(&"main.vec".to_string())
                && g.nodes.contains(&"a.h".to_string())
                && g.nodes.contains(&"b.h".to_string()),
            "",
        );
        set.add("C14-图-边登记", g.edges.len() == 2, "");
        set.add(
            "C14-图-邻接表",
            g.children_of("main.vec") == vec!["a.h"]
                && g.children_of("a.h") == vec!["b.h"]
                && g.children_of("b.h").is_empty(),
            "",
        );
    }
    // 成功产物必无环（成环已在解析期报错，图自检兜一层）
    {
        let s = chain_source();
        let cfg = IncludeConfig::new(16);
        let (_, g, _, _) =
            resolve_includes(&s, &cfg, "main.vec", s.get("main.vec").unwrap().as_str()).unwrap();
        set.add("C14-图-成功产物无环", g.is_acyclic(), "");
    }
    // 重复登记同一条边不重复计数（图的确定性）
    {
        let mut g = IncludeGraph::default();
        g.add_edge("m", "a", IncludeForm::Quoted, 1);
        g.add_edge("m", "a", IncludeForm::Quoted, 1);
        set.add("C14-图-重复边不重复计数", g.edges.len() == 1, "");
    }
    // 边记形式与行号（构建系统的增量重编依据）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include <a.h>\n");
        s.insert("lib/a.h", "X\n");
        let cfg = IncludeConfig::new(16).with_angle_dir("lib");
        let (_, g, _, _) =
            resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()).unwrap();
        set.add(
            "C14-图-边记形式与行号",
            g.edges.len() == 1 && g.edges[0].form == IncludeForm::Angle && g.edges[0].line == 1,
            "",
        );
    }

    // ---- 判据：缓存裁定 ----

    // Once 语义：同文件只展开一次（第二次命中返回空展开）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n#include \"a.h\"\n");
        s.insert("a.h", "A\n");
        let cfg = IncludeConfig::new(16);
        let (_, _, notes, st) =
            resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()).unwrap();
        set.add(
            "C14-缓-Once只展开一次",
            st.skipped_once == 1 && st.lookups == 1,
            "",
        );
        set.add(
            "C14-缓-跳过出注记不静默",
            notes.iter().any(|n| n.code == "N_INC_SKIPPED_ONCE"),
            "",
        );
    }
    // Repeat 语义：显式选定后每次重展开
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n#include \"a.h\"\n");
        s.insert("a.h", "A\n");
        let cfg = IncludeConfig::new(16).with_once(OnceSemantics::Repeat);
        let (text, _, _, st) =
            resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()).unwrap();
        set.add(
            "C14-缓-Repeat重展开",
            st.skipped_once == 0 && text.matches("A").count() == 2,
            "",
        );
    }
    // 缓存条目登记且指纹随之更新
    {
        let s = chain_source();
        let cfg = IncludeConfig::new(16);
        let mut r = IncludeResolver::new(cfg, &s);
        let _ = r.resolve_root("main.vec", s.get("main.vec").unwrap().as_str());
        let c = r.cache();
        set.add(
            "C14-缓-条目登记",
            c.len() == 2 && c.iter().any(|e| e.path == "a.h") && c.iter().any(|e| e.path == "b.h"),
            "",
        );
    }
    // 缓存失效 → 重展开并标注（判据四原文：重展开并标注）
    //
    // 场景要真实：**同一个解析器**先后解析两次同一个根，其间把被包含文件
    // 内容改掉。第二次必须拿到新内容、出失效注记、失效计数 +1。
    // 早先版本每步都新建解析器，缓存每次都是空的，失效分支根本没被走到，
    // 检查项却写成绿的——那是自检在说谎，不是实现对。
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "OLD\n");
        let root_text = s.get("m.vec").unwrap();
        let cfg = IncludeConfig::new(16);
        let mut r = IncludeResolver::new(cfg, &s);
        let first = r.resolve_root("m.vec", root_text.as_str()).unwrap();
        assert_eq!(first, "OLD\n", "首次展开应拿到旧内容");

        // 同一解析器、缓存已在手，改内容后再解析一次。
        assert!(s.replace("a.h", "NEW\n"), "测试源应可改写");
        let second = r.resolve_root("m.vec", root_text.as_str()).unwrap();
        let stale_note = r.notes().iter().any(|n| n.code == "N_INC_CACHE_STALE");
        set.add(
            "C14-缓-失效重展开并标注",
            second == "NEW\n" && stale_note && r.stats().cache_invalidations == 1,
            "指纹变了就必须重展开并出注记，不许拿旧展开喂下游",
        );
    }
    // 失效后缓存条目被改写成新指纹（不留旧值）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "V1\n");
        let root_text = s.get("m.vec").unwrap();
        let mut r = IncludeResolver::new(IncludeConfig::new(16), &s);
        let _ = r.resolve_root("m.vec", root_text.as_str());
        let fp1 = r
            .cache()
            .iter()
            .find(|e| e.path == "a.h")
            .map(|e| e.fingerprint);
        s.replace("a.h", "V2\n");
        let _ = r.resolve_root("m.vec", root_text.as_str());
        let entry = r.cache().iter().find(|e| e.path == "a.h").cloned();
        let fp2 = entry.as_ref().map(|e| e.fingerprint);
        set.add(
            "C14-缓-指纹变则重展开",
            fp1.is_some() && fp2.is_some() && fp1 != fp2 && fp2 == Some(fingerprint("V2\n")),
            "条目指纹必须跟着内容更新",
        );
    }
    // 缓存命中计数（O(1) 命中面）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n#include \"a.h\"\n");
        s.insert("a.h", "A\n");
        let (_, _, _, st) = resolve_includes(
            &s,
            &IncludeConfig::new(16),
            "m.vec",
            s.get("m.vec").unwrap().as_str(),
        )
        .unwrap();
        set.add("C14-缓-命中计数", st.cache_hits == 1, "");
    }

    // ---- 判据：深度上限 ----

    // 超深报错且指向链顶
    {
        let s = deep_source(40);
        let cfg = IncludeConfig::new(4);
        match resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()) {
            Err(e) => set.add("C14-深-超限报错", e.code == "E_INC_DEPTH_EXCEEDED", ""),
            Ok(_) => set.add("C14-深-超限报错", false, "本应超限却成功了"),
        }
        match resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()) {
            Err(e) => set.add(
                "C14-深-指向链顶",
                e.chain_top.as_ref().map(|t| t.path.as_str()) == Some("m.vec"),
                "链顶= 包含链的起点（根），拆递归从根着手",
            ),
            Ok(_) => set.add("C14-深-指向链顶", false, "本应超限却成功了"),
        }
        match resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()) {
            Err(e) => set.add("C14-深-链快照非空", !e.chain.is_empty(), ""),
            Ok(_) => set.add("C14-深-链快照非空", false, "本应超限却成功了"),
        }
    }

    // ---- 判据：零静默 ----

    // 找不到时报错三要素齐备
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"nope.h\"\n");
        match resolve_includes(
            &s,
            &IncludeConfig::new(16),
            "m.vec",
            s.get("m.vec").unwrap().as_str(),
        ) {
            Err(e) => set.add("C14-显性-找不到三要素齐备", e.is_complete(), ""),
            Ok(_) => set.add("C14-显性-找不到三要素齐备", false, "本应报错却成功了"),
        }
    }
    // 成环时报错三要素齐备
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#include \"a.h\"\n");
        s.insert("a.h", "#include \"a.h\"\n");
        match resolve_includes(
            &s,
            &IncludeConfig::new(16),
            "m.vec",
            s.get("m.vec").unwrap().as_str(),
        ) {
            Err(e) => set.add("C14-显性-成环三要素齐备", e.is_complete(), ""),
            Ok(_) => set.add("C14-显性-成环三要素齐备", false, "本应报环却成功了"),
        }
    }
    // 处置方向相反的状态不共用码（判据纪律：找不到 ≠ 成环 ≠ 超深）
    {
        set.add(
            "C14-显性-错误码不合并",
            "E_INC_NOT_FOUND" != "E_INC_CYCLE" && "E_INC_CYCLE" != "E_INC_DEPTH_EXCEEDED",
            "",
        );
    }
    // 空操作数 / 未闭合引号 / 未闭合尖括号各走独立码
    {
        set.add(
            "C14-显性-空操作数",
            parse_include_operand("  ").is_err(),
            "",
        );
        set.add(
            "C14-显性-未闭合引号",
            matches!(parse_include_operand("\"a.h"), Err((c, _)) if c == "E_INC_UNCLOSED_QUOTE"),
            "",
        );
        set.add(
            "C14-显性-未闭合尖括号",
            matches!(parse_include_operand("<a.h"), Err((c, _)) if c == "E_INC_UNCLOSED_ANGLE"),
            "",
        );
        set.add(
            "C14-显性-两种形态解析",
            matches!(
                parse_include_operand("\"a.h\""),
                Ok((IncludeForm::Quoted, "a.h"))
            ) && matches!(
                parse_include_operand("<a.h>"),
                Ok((IncludeForm::Angle, "a.h"))
            ),
            "",
        );
    }
    // 路径规范化：消解 . 与 ..（环检测的前置）
    {
        set.add(
            "C14-显性-路径规范化",
            normalize("a/./b/../c").as_deref() == Some("a/c")
                && normalize("./a").as_deref() == Some("a")
                && normalize("../a").is_none(),
            "",
        );
    }
    // 指纹确定性：同内容同指纹（缓存裁定的前提）
    {
        set.add(
            "C14-显性-指纹确定性",
            fingerprint("abc") == fingerprint("abc") && fingerprint("abc") != fingerprint("abd"),
            "",
        );
    }
    // 非 include 指令原样透传（不越权求值宏与条件编译）
    {
        let s = MemorySource::new();
        s.insert("m.vec", "#define X 1\n#include \"a.h\"\n");
        s.insert("a.h", "A\n");
        let cfg = IncludeConfig::new(16);
        let (text, _, _, _) =
            resolve_includes(&s, &cfg, "m.vec", s.get("m.vec").unwrap().as_str()).unwrap();
        set.add(
            "C14-显性-非include指令透传",
            text.starts_with("#define X 1") && text.contains("A"),
            "",
        );
    }
    // 展开文本顺序确定（同输入同输出，可对拍）
    {
        let s = chain_source();
        let cfg = IncludeConfig::new(16);
        let a = run(&s, &cfg, "main.vec");
        let b = run(&s, &cfg, "main.vec");
        set.add("C14-显性-展开顺序确定", a == b, "");
    }
    // 产出字节计量（性能判据面）
    {
        let s = chain_source();
        let cfg = IncludeConfig::new(16);
        let (_, _, _, st) =
            resolve_includes(&s, &cfg, "main.vec", s.get("main.vec").unwrap().as_str()).unwrap();
        set.add(
            "C14-显性-产出字节计量",
            st.emitted_bytes > 0 && st.peak_depth >= 2 && st.directives == 2,
            "",
        );
    }

    set
}

#[cfg(test)]
mod red_report {
    use super::*;
    #[test]
    fn report_red_items() {
        let set = run_vec14_checks();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("RED: {} | {}", c.name, c.detail);
                }
            }
        }
        println!("total={} dropped={}", set.len(), set.dropped());
    }
}
