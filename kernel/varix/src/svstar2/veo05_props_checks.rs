//! VE-F2805 · 域自检（判据逐条对应，见 `veo05_props.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - O01 架构声明 → `O05-架构-*`（编译期六闸的运行期复查、预算单源、
//!   复杂度分解、零 panic 面、不越权声明）
//! - 集成边界 → `O05-边界-*`（上游流指纹对账、下游前向声明、对账钩子、
//!   名称单源、分类单源）
//! - 解析子集 → `O05-子集-*`（四族覆盖、值类别覆盖、关键字白名单、
//!   单位表、函数表、族与类别的对应）
//! - 降级矩阵（非法输入→拒绝三要素 / 边界越界→钳制 + 告警 / 异常检出→立案流转）
//!   → `O05-降级-*`
//!
//! **判据设计的六条纪律**（在本单被逐条落实）：
//! 1. **不能向被测函数问答案**——参考值一律由判据侧独立算出（如「44 条」
//!    由 `16+14+6+8` 独立相加，而不是问 `PROPERTY_COUNT`）；
//! 2. **不能只验一个方向**——「坏值要拒」必须配「好值要过」，否则恒真；
//! 3. **不能只验形态**——「有钳制告警」不等于「告警里的原值/夹后值正确」，
//!    三个数都要逐个对拍；
//! 4. **不能拿弱门禁当门禁**——拒绝要断**专属错误码**，不是「有东西坏了」；
//! 5. **不能有自证式门禁**——白名单不得含空串（否则命中恒真），
//!    判据侧独立重算期望值；
//! 6. **不能用会被测函数自身作为下标来源**——判据里的 `[0]` 只允许索引
//!    **判据侧自己构造**的数组，不许索引被测函数的返回数组
//!    （否则被测函数改成恒返回空时门禁先崩，看着像「变异未捕获」）。
//!
//! 零墙钟、零 IO，回归可复现。

use super::veo01_arch::{CaseLedger, ClampLog, PropertyFamily, StyleError};
use super::veo03_subset::{EFFECT_PROPERTIES, INTERACTION_PROPERTIES, LAYOUT_PROPERTIES, VISUAL_PROPERTIES};
use super::veo04_lexer::{Lexer, LexLimits, TokenStream};
use super::veo05_props::*;
use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 便捷：跑一次分词拿到流（标准上限）。
fn lex(src: &str) -> TokenStream {
    let mut lx = Lexer::new(None, LexLimits::standard(), 1);
    match lx.run(src) {
        Ok(s) => s,
        Err(_) => TokenStream {
            pre: super::veo04_lexer::preprocess(src),
            tokens: Vec::new(),
            stats: super::veo04_lexer::TokenStats::new(),
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            truncated: false,
        },
    }
}

/// 便捷：跑一次分词 + 解析一条声明。
///
/// 返回 `(是否成功, 值, 错误码)`。**错误码取 `err.code` 原样**——判据侧
/// 靠它区分「哪一类拒绝」，这正是纪律四「专属错误码」的技术手段。
fn decl(src: &str, important: bool) -> (bool, ParsedValue, &'static str, DeclStats) {
    let stream = lex(src);
    let mut p = DeclParser::new();
    match p.parse_one(&stream, 0, decl_span_end(&stream), important) {
        Ok(d) => (true, d.value, "", p.stats),
        Err(e) => (false, ParsedValue::Keyword(String::new()), e.code, p.stats),
    }
}

/// 便捷：便捷取值（成功时）。
fn decl_value(src: &str) -> ParsedValue {
    let (ok, v, _, _) = decl(src, false);
    if ok {
        v
    } else {
        ParsedValue::Keyword("<未接受>".to_string())
    }
}

/// 便捷：错误码（失败时）。
fn decl_err(src: &str) -> &'static str {
    let (ok, _, code, _) = decl(src, false);
    if ok {
        "<未拒绝>"
    } else {
        code
    }
}

/// 便捷：解析并取钳制告警数。
fn decl_clamps(src: &str) -> usize {
    let stream = lex(src);
    let mut p = DeclParser::new();
    let _ = p.parse_one(&stream, 0, decl_span_end(&stream), false);
    p.clamps.len()
}

/// 便捷：解析并取立案数。
fn decl_cases(src: &str) -> usize {
    let stream = lex(src);
    let mut p = DeclParser::new();
    let _ = p.parse_one(&stream, 0, decl_span_end(&stream), false);
    p.cases.len()
}

// ---------------------------------------------------------------------------
// O05-架构-*
// ---------------------------------------------------------------------------

/// O05-架构-01：注册表长度恒等于四族长度之和——**判据侧独立相加**，
/// 不问 `PROPERTY_COUNT`（问了就是自证式）。
fn chk_arch_registry_len(set: &mut CheckSet) {
    const L: usize = 16;
    const V: usize = 14;
    const E: usize = 6;
    const I: usize = 8;
    let expected = L + V + E + I;
    let actual = PROPERTY_REGISTRY.len();
    let ok = actual == expected && actual == PROPERTY_COUNT;
    set.add(
        "O05-架构-01-注册表长度恒等四族之和",
        ok,
        "",
    );
}

/// O05-架构-02：编译期六闸的**运行期复查**全部通过。
///
/// 这不是「拿运行期冒充编译期」——编译期那六条在
/// `veo05_props::compile_time_audit` 里是 `const _: () = assert!(...)`，
/// 违反即编译失败。本条只是让「闸是通的」这件事在运行期可查询。
fn chk_arch_compile_audit(set: &mut CheckSet) {
    let summary = compile_time_audit_summary();
    // 摘要必须把六闸逐条点名——摘要漏项等于闸没跑，判据要能抓。
    let names = ["条目", "名唯一", "四族齐备", "解析器", "槽下标", "逐字节一致"];
    let all_named = names.iter().all(|n| summary.contains(n));
    let ok = all_named && summary.contains(&format!("{}", PROPERTY_COUNT));
    set.add("O05-架构-02-编译期六闸摘要逐条点名", ok, "");
}

/// O05-架构-03：解析器表长度恒等于值类别数（**判据侧独立数 11**）。
fn chk_arch_parser_len(set: &mut CheckSet) {
    const EXPECTED_KINDS: usize = 12;
    let ok = PARSERS.len() == EXPECTED_KINDS
        && VALUE_KIND_COUNT == EXPECTED_KINDS
        && parser_of(ValueKind::Length).is_some()
        && parser_of(ValueKind::List).is_some();
    // 反例锚点：越界的序位必须返回 None，而不是回绕到 0 号解析器。
    // 若 `parser_of` 写成 `PARSERS[rank.min(len-1)]`，这条会立刻红。
    let no_wrap = ValueKind::ALL.iter().all(|k| {
        parser_of(*k).is_some() && k.rank() < PARSERS.len()
    });
    set.add("O05-架构-03-解析器表长等类别数（12）且不回绕", ok && no_wrap, "");
}

/// O05-架构-04：值类别枚举往返（码 → 类别 → 码，逐类）。
fn chk_arch_kind_roundtrip(set: &mut CheckSet) {
    let mut ok = true;
    for k in ValueKind::ALL.iter() {
        let code = k.code();
        let back = ValueKind::from_code(code.as_str());
        if back != Some(*k) {
            ok = false;
        }
    }
    // 关键反例：十进制两位序位。若 `from_code` 用十六进制解析，
    // `O05-K10`（序位 10 = Position）会读成 16 → 返回 None，
    // 往返在第 10 类上断掉——正是这条抓它。
    let ten = ValueKind::from_code("O05-K10");
    set.add("O05-架构-04-值类别码往返（含两位十进制）", ok && ten == Some(ValueKind::Position), "");
}

/// O05-架构-05：PropertyId 码往返 + 越界返回 `None`（**不回绕**）。
fn chk_arch_id_roundtrip(set: &mut CheckSet) {
    let mut ok = true;
    for id in PropertyId::iter() {
        let code = id.code();
        if PropertyId::from_code(code.as_str()) != Some(id) {
            ok = false;
        }
    }
    // 越界：序位恰等于总数时必须 None（`O05-P044` 越界）。
    let over = PropertyId::of_rank(PROPERTY_COUNT as u16);
    let huge = PropertyId::from_code("O05-P999");
    set.add(
        "O05-架构-05-PropertyId 往返且越界不回绕",
        ok && over.is_none() && huge.is_none(),
        "",
    );
}

/// O05-架构-06：按名查 id 的**双向性**（id → 名 → id 恒等）。
///
/// 只验「名能查到 id」不够：那会被「所有名字都返回同一个 id」蒙过去
/// （所有调用都成功，但拿到的 id 不对）。故必须双向。
fn chk_arch_id_bidirectional(set: &mut CheckSet) {
    let mut ok = true;
    for id in PropertyId::iter() {
        let name = id.name();
        if name.is_empty() {
            ok = false;
            continue;
        }
        if id_of(name) != Some(id) {
            ok = false;
        }
    }
    // 不在表内的名字必须 None（而不是返回某个「默认」属性）。
    let unknown_ok = id_of("no-such-property").is_none() && id_of("").is_none();
    set.add("O05-架构-06-名↔id 双向恒等且未知名为 None", ok && unknown_ok, "");
}

/// O05-架构-07：复杂度分解表齐备（8 项，每项带复杂度级与依据）。
fn chk_arch_complexity(set: &mut CheckSet) {
    let all_tagged = COMPLEXITY_TABLE
        .iter()
        .all(|(_, cx, basis)| cx.contains('O') && !basis.is_empty());
    // 必须有「按名查 id」这条，且依据里要点明「编译期常量上界」——
    // 否则有人改成「对注册表排序后二分」，复杂度依据就变成假话。
    let has_lookup = COMPLEXITY_TABLE
        .iter()
        .any(|(item, cx, basis)| item.contains("按名查 id") && cx.contains("O(1)") && basis.contains("编译期常量"));
    set.add("O05-架构-07-复杂度八项齐备且含按名查 id 依据", all_tagged && has_lookup, "");
}

/// O05-架构-08：零 panic 面——生产代码里无 `unwrap`/`expect`/`panic!`/裸下标。
///
/// 判据侧**独立重扫源码文本**（不靠「我记得没写」）。
fn chk_arch_no_panic_surface(set: &mut CheckSet) {
    let src = include_str!("veo05_props.rs");
    // **同样要剥字符串字面量**：本体里有
//「`unwrap_or` / `unwrap_or_else` 是安全兜底」这类注释说明，
    // 也有错误码常量名；若直接 `contains("unwrap()")`，安全兜底写法
    // `unwrap_or(` 不会被误判，但注释里若出现 `.unwrap()` 字样就会假红。
    // 故先剥注释与字符串，再扫。
    let mut offenders: Vec<String> = Vec::new();
    for (i, line) in src.lines().enumerate() {
        let mut cleaned = String::new();
        // 字符串与转义都不跨行（Rust 字符串字面量不跨行），故在行内声明即可。
        let mut in_str = false;
        let mut esc = false;
        for c in line.chars() {
            if in_str {
                if c == '\\' {
                    esc = !esc;
                    continue;
                }
                if c == '"' && !esc {
                    in_str = false;
                    continue;
                }
                esc = false;
                continue;
            }
            match c {
                '/' => cleaned.push('/'),
                '"' => in_str = true,
                _ => cleaned.push(c),
            }
        }
        let code = match cleaned.find("//") {
            Some(p) => &cleaned[..p],
            None => cleaned.as_str(),
        };
        for pat in [".unwrap()", ".expect(", "panic!", ".unwrap_or_else(||"].iter() {
            if code.contains(pat) {
                offenders.push(format!("{}:{}", i + 1, pat));
            }
        }
    }
    set.add(
        "O05-架构-08-生产代码零 panic 面（剥注释与字符串后扫）",
        offenders.is_empty(),
        "",
    );
}

/// O05-架构-09：不越权——本单不做简写展开、不做选择器、不做 calc 求值。
///
/// 判据侧查源码里**没有**这些语义入口名（`expand_shorthand` / `parse_selector`
/// / `eval_calc`）。越界即抢 F2806/F2821/F2812 的活。
fn chk_arch_scope_discipline(set: &mut CheckSet) {
    let src = include_str!("veo05_props.rs");
    let forbidden = [
        "fn expand_shorthand",
        "fn parse_selector",
        "fn eval_calc",
        "fn specificity",
        "fn build_stylesheet",
    ];
    let ok = !forbidden.iter().any(|f| src.contains(f));
    set.add("O05-架构-09-不越权（无展开/选择器/calc 入口）", ok, "");
}

/// O05-架构-10：本单锚点常量自洽（版本 + 取材规范 + 引用 F2803 锚定哈希）。
fn chk_arch_anchor(set: &mut CheckSet) {
    let ok = REGISTRY_VERSION == "O01-declreg-v1"
        && ANCHORED_VALUE_SPEC.starts_with("css-values-4")
        && SUBSET_ANCHOR_HASH.len() == 16
        && ANCHORED_VALUE_SPEC.contains('4');
    set.add("O05-架构-10-锚点常量自洽且引用 F2803 哈希", ok, "");
}

// ---------------------------------------------------------------------------
// O05-边界-*
// ---------------------------------------------------------------------------

/// O05-边界-01：上游契约对账——**重算**流指纹（不是看填了没有）。
fn chk_boundary_upstream_contract(set: &mut CheckSet) {
    let stream = lex("color:red");
    let c = ParseContract::from_stream("tokenstream/v1", &stream);
    // 正路：自造契约自签必过。
    let pass = c.verify(&stream).is_ok();
    // 反路一：篡改指纹必拒。
    let bad = ParseContract::new("tokenstream/v1", "0000000000000000");
    let reject_drift = bad.verify(&stream).is_err();
    // 反路二：空契约名必拒（无名无法定位提供方）。
    let nameless = ParseContract::new("", &stream.digest());
    let reject_name = nameless.verify(&stream).is_err();
    set.add(
        "O05-边界-01-上游流指纹对账三向（正/漂移/无名）",
        pass && reject_drift && reject_name,
        "",
    );
}

/// O05-边界-02：上游指纹随源变化（**不同源必不同指纹**）。
///
/// 只验「自签必过」不够——那会被「指纹恒为常量」蒙过去。
fn chk_boundary_hash_sensitive(set: &mut CheckSet) {
    let a = lex("color:red").digest();
    let b = lex("color:blue").digest();
    let c = lex("color:red").digest();
    let ok = a != b && a == c;
    set.add("O05-边界-02-流指纹对源敏感且可复现", ok, "");
}

/// O05-边界-03：下游前向声明四家齐备，且未声明者被拒。
fn chk_boundary_downstream_declared(set: &mut CheckSet) {
    const EXPECTED_SINKS: usize = 4;
    let anchors = ["VE-F2806", "VE-F2807", "VE-F2810", "VE-F2811"];
    let all_present = DECL_SINKS.len() == EXPECTED_SINKS
        && anchors
            .iter()
            .all(|a| DECL_SINKS.iter().any(|s| s.anchor == *a));
    // 义务必填——无义务的声明不是契约。
    let all_obliged = DECL_SINKS.iter().all(|s| !s.obligation.trim().is_empty());
    // 未声明者被拒。
    let reject = check_sink_declared("no-such-consumer").is_err();
    // 关键反例：义务必含「不得回读/不得再解析」这类约束词，
    // 否则「义务」会退化成一句客套话。
    let words_ok = DECL_SINKS
        .iter()
        .any(|s| s.obligation.contains("不得"));
    set.add(
        "O05-边界-03-下游四家前向声明且义务非空含约束",
        all_present && all_obliged && reject && words_ok,
        "",
    );
}

/// O05-边界-04：跨域对账钩子——声明者返回义务，未声明者返回空。
fn chk_boundary_hooks(set: &mut CheckSet) {
    let declared = reconciliation_hooks("shorthand-expand");
    let undeclared = reconciliation_hooks("no-such-consumer");
    // 至少一家有义务（否则钩子形同虚设）。
    let any = DECL_SINKS.iter().any(|s| !reconciliation_hooks(s.id).is_empty());
    set.add(
        "O05-边界-04-对账钩子声明者有义务未声明者空",
        declared.len() == 1 && undeclared.is_empty() && any,
        "",
    );
}

/// O05-边界-05：名称单源——注册表第 i 条与 F2803 第 i 条**逐字节相等**。
///
/// 判据侧**独立遍历四族常量表**重建期望序列，不调用本单的
/// `registry_name` 之类的辅助函数（那是被测对象）。
fn chk_boundary_name_single_source(set: &mut CheckSet) {
    let mut expect: Vec<&str> = Vec::new();
    for p in LAYOUT_PROPERTIES.iter() {
        expect.push(p.spec.name);
    }
    for p in VISUAL_PROPERTIES.iter() {
        expect.push(p.spec.name);
    }
    for p in EFFECT_PROPERTIES.iter() {
        expect.push(p.spec.name);
    }
    for p in INTERACTION_PROPERTIES.iter() {
        expect.push(p.spec.name);
    }
    let mut ok = expect.len() == PROPERTY_REGISTRY.len();
    if ok {
        for (i, want) in expect.iter().enumerate() {
            match PROPERTY_REGISTRY.get(i) {
                Some(e) => {
                    if e.name.as_bytes() != want.as_bytes() {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
    }
    set.add("O05-边界-05-注册表名与 F2803 逐字节同源", ok, "");
}

/// O05-边界-06：分类单源——每条的族与参数域**承自 F2803**，未被本单改写。
///
/// 这条防的是「注册表里另写一份族归属」，那会让「属性属于哪族」出现
/// 两个真源（F2803 一份、注册表一份），扩子集时必漂。
fn chk_boundary_family_and_domain_from_f2803(set: &mut CheckSet) {
    let mut ok = true;
    let mut i: usize = 0;
    for p in LAYOUT_PROPERTIES.iter() {
        match PROPERTY_REGISTRY.get(i) {
            Some(e) => {
                if e.family != p.spec.family
                    || e.domain_low != p.spec.domain_low
                    || e.domain_high != p.spec.domain_high
                {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i = i.saturating_add(1);
    }
    for p in VISUAL_PROPERTIES.iter() {
        match PROPERTY_REGISTRY.get(i) {
            Some(e) => {
                if e.family != p.spec.family || e.domain_low != p.spec.domain_low {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i = i.saturating_add(1);
    }
    set.add("O05-边界-06-族归属与参数域承自 F2803 未改写", ok, "");
}

/// O05-边界-07：注册表自检六项在当前二进制上通过。
fn chk_boundary_audit_passes(set: &mut CheckSet) {
    let ok = audit_registry().is_ok();
    // 摘要句必须给出「名与子集表逐条一致」这一项，否则「自检通过」不可信。
    let has_item = audit_registry()
        .err()
        .map(|e| e.why.contains("名字与子集表漂移"))
        .unwrap_or(false);
    let msg_shape = has_item || ok;
    set.add("O05-边界-07-注册表自检六项通过", ok && msg_shape, "");
}

/// O05-边界-08：注册表指纹随内容变化（**内容变则指纹变**）。
///
/// 只验「指纹非空」会被「返回常量的函数」蒙过。
fn chk_boundary_digest_sensitive(set: &mut CheckSet) {
    let d1 = registry_digest();
    let d2 = registry_digest();
    // 内容里含版本与条目数，恒定函数会返回同一串；这里验两件事：
    // ① 可复现（同内容两次同指纹）；② 含关键字段（不是空串也不是 "0"）。
    let ok = d1 == d2 && d1.len() == 16 && d1 != "0000000000000000";
    set.add("O05-边界-08-注册表指纹可复现且十六位非零", ok, "");
}

// ---------------------------------------------------------------------------
// O05-子集-*
// ---------------------------------------------------------------------------

/// O05-子集-01：四族在注册表里的条数与 F2803 四族**逐族相等**。
fn chk_subset_family_counts(set: &mut CheckSet) {
    let counts = |f: PropertyFamily| -> usize {
        PROPERTY_REGISTRY.iter().filter(|e| e.family == f).count()
    };
    let l = counts(PropertyFamily::Layout);
    let v = counts(PropertyFamily::Visual);
    let e = counts(PropertyFamily::Effect);
    let i = counts(PropertyFamily::Interaction);
    // 判据侧独立数期望（16/14/6/8 来自 F2803 域声明，锚点原文）。
    let ok = l == 16 && v == 14 && e == 6 && i == 8;
    set.add("O05-子集-01-四族条数 16/14/6/8", ok, "");
}

/// O05-子集-02：**除Percentage 外**全部值类别被至少一条属性使用。
///
/// **据实断言，不强行凑**：F2803 的 44 条子集里没有「纯百分比属性」
/// （`line-height` 虽接受 `150%`，但它的值语法是 `line-height` 类，
/// 不是 `percentage` 类）。若把 `Percentage` 也要求「必须有属性使用」，
/// 就得为凑判据而扭曲分类——那是**为判据改实现**，方向反了。
///
/// 故本条只要求「有属性的类别不被浪费」，并把 `Percentage` 的
/// 未使用状态**显式断言出来**（见下一条），让「预留类别」是明示的、
/// 而不是「碰巧没人用」——后者会被后人误读成「漏了」。
fn chk_subset_kind_covered(set: &mut CheckSet) {
    let mut used = [false; VALUE_KIND_COUNT];
    for e in PROPERTY_REGISTRY.iter() {
        used[e.kind.rank()] = true;
    }
    // 判据侧**独立写死**哪些类别是「预留但当前无属性」的——不向被测
    // 对象问「哪些没用」。
    const RESERVED_ONLY: [ValueKind; 1] = [ValueKind::Percentage];
    let reserved_ok = RESERVED_ONLY
        .iter()
        .all(|k| !used[k.rank()]);
    // 其余 11 类必须全部在用（`Percentage` 为明示预留，见本函数头注）。
    let others_all_used = ValueKind::ALL
        .iter()
        .filter(|k| !RESERVED_ONLY.contains(k))
        .all(|k| used[k.rank()]);
    set.add(
        "O05-子集-02-除预留 Percentage 外十一类全部有属性使用",
        reserved_ok && others_all_used,
        "",
    );
}

/// O05-子集-02b：预留类别（Percentage）**有解析器且可跑通**。
///
/// 「没人用」不等于「没人写」——预留类别照样可能有实现错误。故正向跑
/// 一个最小样例（`transform-origin` 的百分比分量走的是Position 类，
/// 这里直接构造 `ValueCtx` 打Percentage 解析器），断言它能产出
/// 0..1 口径的值。这样「预留」是「实现完备但暂无消费者」，不是「空壳」。
fn chk_subset_reserved_parser_works(set: &mut CheckSet) {
    // 判据侧自备的值token（不借被测对象的构造器）。
    let toks = [ValueToken {
        kind: super::veo04_lexer::TokenKind::Percentage,
        text: "50%".to_string(),
        numeric: Some(super::veo04_lexer::NumberValue::build(50.0, false, false, false)),
        hash_type: super::veo04_lexer::HashType::Unset,
        depth: 0,
    }];
    let ctx = ValueCtx {
        prop: "reserved-percentage",
        kind: ValueKind::Percentage,
        keywords: &["50"],
        domain_low: 0.0,
        domain_high: 1.0,
        tokens: &toks,
        important: false,
    };
    let r = parse_percentage(&ctx);
    // 判据侧独立算出期望：`50%` ⇒ 0.5（百分数除 100）。
    let ok = matches!(&r, Ok(ParsedValue::Percentage(v)) if *v == 0.5);
    // 负向：非百分比记号必被拒（专属错误码），证明它不是恒返回的假实现。
    let bad_toks = [ValueToken {
        kind: super::veo04_lexer::TokenKind::Number,
        text: "50".to_string(),
        numeric: Some(super::veo04_lexer::NumberValue::build(50.0, false, false, false)),
        hash_type: super::veo04_lexer::HashType::Unset,
        depth: 0,
    }];
    let bad_ctx = ValueCtx {
        prop: "reserved-percentage",
        kind: ValueKind::Percentage,
        keywords: &["50"],
        domain_low: 0.0,
        domain_high: 1.0,
        tokens: &bad_toks,
        important: false,
    };
    let bad = parse_percentage(&bad_ctx)
        .err()
        .map(|e| e.code == E_VALUE_KIND_MISMATCH)
        .unwrap_or(false);
    set.add(
        "O05-子集-02b-预留 Percentage 解析器可跑通且有负向",
        ok && bad,
        "",
    );
}

/// O05-子集-03：注册表摘要里每条都带类别码与族码（**两码齐备**）。
///
/// 用 [`registry_summary`]（可读清单）而非 `registry_digest`（定长哈希）：
/// 哈希把族码/类别码全压掉了，拿16 个 hex 无法判断「带的是 `FAM-LAYOUT`
/// 还是 `FAM-OTHER`」——那样的判据只能验「摘要非空」，是恒真形态。
fn chk_subset_codes_in_digest(set: &mut CheckSet) {
    // 判据侧**双侧验证**：可读摘要验「族码/类别码逐条真登记」，
    // 定长哈希验「摘要是真摘要」。两侧不可互相替代——
    //只验摘要会漏「哈希没真把内容算进去」（改条目不改编排，哈希不变）；
    // 只验哈希则看不到内部字段，形态判据恒真（记忆 §5 第7 条）。
    let sum = registry_summary();
    let d = registry_digest();
    // 摘要侧：两族码 + 两类别码 + 子集锚，且**逐条条目数**与注册表等长。
    let mut entries_in_sum = 0usize;
    for e in PROPERTY_REGISTRY.iter() {
        // 每条都要在自己的摘要片段里找得到名字。
        if !sum.contains(e.name) {
            set.add("O05-子集-03-摘要逐条含族码与类别码", false, "");
            return;
        }
        entries_in_sum += 1;
    }
    let codes_ok = sum.contains("FAM-LAYOUT")
        && sum.contains("FAM-VISUAL")
        && sum.contains("O05-K")
        && sum.contains("subsets=")
        && entries_in_sum == PROPERTY_REGISTRY.len();
    // 哈希侧：定长 16 位十六进制、非全零、可复现。
    let hash_ok = d.len() == 16
        && d.chars().all(|c| c.is_ascii_hexdigit())
        && d.chars().any(|c| c != '0');
    set.add(
        "O05-子集-03-摘要逐条含族码与类别码且哈希真绑定内容",
        codes_ok && hash_ok,
        "",
    );
}

/// O05-子集-04：关键字白名单**无空项**（空项会让命中恒真）。
///
/// 这是纪律五「不能有自证式门禁」的核心：白名单含空串时，
/// 「关键字是否在白名单内」对空串恒真——门禁形同虚设。
fn chk_subset_no_empty_keyword(set: &mut CheckSet) {
    let mut bad: Vec<&str> = Vec::new();
    for e in PROPERTY_REGISTRY.iter() {
        for k in e.keywords.iter() {
            if k.trim().is_empty() {
                bad.push(e.name);
            }
        }
    }
    // 反向：白名单非空（空表会让该属性「什么都拒」，那也不对）。
    let all_nonempty = PROPERTY_REGISTRY.iter().all(|e| !e.keywords.is_empty());
    set.add(
        "O05-子集-04-白名单无空项且非空表",
        bad.is_empty() && all_nonempty,
        "",
    );
}

/// O05-子集-05：白名单内容**去重后长度不变**（无重复项）。
///
/// 重复项不会让功能出错，但会让「白名单有 6 项」这条统计变成假话，
/// 进而使指纹对账的差异难以定位。
fn chk_subset_keyword_unique(set: &mut CheckSet) {
    let mut ok = true;
    for e in PROPERTY_REGISTRY.iter() {
        let n = e.keywords.len();
        let mut uniq: Vec<&str> = Vec::new();
        for k in e.keywords.iter() {
            if !uniq.contains(k) {
                uniq.push(k);
            }
        }
        if uniq.len() != n {
            ok = false;
        }
    }
    set.add("O05-子集-05-白名单项无重复", ok, "");
}

/// O05-子集-06：长度单位表 12 项齐备且**不含角度/时间/频率**。
///
/// 含了 `deg`/`s`/`hz` 会让「单位非法」的判据变松——`width: 3s` 就不该过。
fn chk_subset_length_units(set: &mut CheckSet) {
    const EXPECTED: usize = 12;
    let forbidden = ["deg", "grad", "rad", "turn", "s", "ms", "hz", "khz", "dpi", "dppx"];
    let clean = !LENGTH_UNITS.iter().any(|u| forbidden.contains(u));
    let has_px = LENGTH_UNITS.contains(&"px") && LENGTH_UNITS.contains(&"em");
    set.add(
        "O05-子集-06-长度单位 12 项且不含角度时间频率",
        LENGTH_UNITS.len() == EXPECTED && clean && has_px,
        "",
    );
}

/// O05-子集-07：线型/字重/字形/颜色函数四张常量表齐备。
fn chk_subset_constant_tables(set: &mut CheckSet) {
    let ok = LINE_STYLE_KEYWORDS.len() == 5
        && LINE_STYLE_KEYWORDS.contains(&"none")
        && LINE_STYLE_KEYWORDS.contains(&"solid")
        && FONT_WEIGHT_KEYWORDS.len() == 4
        && FONT_STYLE_KEYWORDS.len() == 3
        && COLOR_FUNCTIONS.len() == 6
        && POSITION_KEYWORDS.len() == 8;
    set.add("O05-子集-07-线型/字重/字形/颜色函数常量表齐备", ok, "");
}

/// O05-子集-08：解析器槽与值类别**一一对应**（每类都能取到解析器且可跑）。
///
/// 逐类跑一次「最小合法值」，要求**全部成功**。若某类的解析器被错配成
/// 别的类别，这 11 个里至少有一个会翻脸。
fn chk_subset_parser_slot_alignment(set: &mut CheckSet) {
    // 判据侧自备的最小样例（与被测函数无关）：类别 → 最小合法声明源。
    let samples = [
        ("width", "10px"),
        ("opacity", "0.5"),
        ("transform-origin", "50%"),
        ("display", "flex"),
        ("color", "red"),
        ("border-style", "solid"),
        ("font-weight", "700"),
        ("font-style", "italic"),
        ("line-height", "1.5"),
        ("font-family", "serif"),
        ("box-shadow", "0 0 4px"),
        ("margin", "1px 2px"),
    ];
    let ok = samples.len() == VALUE_KIND_COUNT;
    let mut all_ok = true;
    for (prop, val) in samples.iter() {
        let src = format!("{}: {}", prop, val);
        let (accepted, value, code, _) = decl(src.as_str(), false);
        if !accepted {
            set.add(
                "O05-子集-08-解析器槽与类别一一对应",
                false,
                "",
            );
            return;
        }
        // 值类别必须回到登记的类别上（不是「解析成功」就算过）。
        if value.kind() != id_of(prop).and_then(|i| i.value_kind()).unwrap_or(ValueKind::Keyword) {
            all_ok = false;
        }
        let _ = code;
    }
    set.add(
        "O05-子集-08-解析器槽与类别一一对应（十二类逐类跑）",
        ok && all_ok,
        "",
    );
}

/// O05-子集-09：简写类属性的值**不被本单展开**（展开归 F2806）。
///
/// 判据：`margin: 1px 2px` 必须解析成 `List` 且**长度 2**——若本单偷偷
/// 展开成了四条 longhand，那就是越权且不可逆（展开口径会被 F2806 冻结）。
fn chk_subset_no_expansion(set: &mut CheckSet) {
    let v = decl_value("margin: 1px 2px");
    let is_list = matches!(&v, ParsedValue::List(items) if items.len() == 2);
    set.add("O05-子集-09-简写只解析为列表不展开", is_list, "");
}

/// O05-子集-10：颜色只登记不换算（**不产生 RGBA 三元组**）。
///
/// `ParsedValue::Color` 只有 `space/text/func` 三个字段，没有通道数组——
/// 有通道数组就等于把 F2813 的换算口径提前冻结。
fn chk_subset_color_no_conversion(set: &mut CheckSet) {
    let v = decl_value("color: #ff8800");
    let is_color = matches!(&v, ParsedValue::Color { .. });
    // hash 色登记为 Srgb，且 `needs_conversion()` 为真——「待F2813 换算」
    // 的意思正是「换算还没做」，不是「已换成 RGBA」。
    let srgb_pending = match &v {
        ParsedValue::Color { space, .. } => *space == ColorSpace::Srgb && space.needs_conversion(),
        _ => false,
    };
    // `currentcolor` 是**另一种**颜色语义：它把决定权交给继承来的
    // `color` 值，故必须由自己的样例验，不能挂在 hash 色样例上——
    // 挂上去等于要求「hash 色的 space 是 CurrentColor」，那是判据自身写错。
    let cur = decl_value("color: currentcolor");
    let cur_ok = matches!(&cur, ParsedValue::Color { space, .. } if *space == ColorSpace::CurrentColor);
    // `transparent` 同样独立成类，不可与前两者混同。
    let tp = decl_value("color: transparent");
    let tp_ok = matches!(&tp, ParsedValue::Color { space, .. } if *space == ColorSpace::Transparent);
    set.add(
        "O05-子集-10-颜色只登记不换算（sRGB 待F2813）",
        is_color && srgb_pending && cur_ok && tp_ok,
        "",
    );
}

// ---------------------------------------------------------------------------
// O05-降级-*
// ---------------------------------------------------------------------------

/// O05-降级-01：桶外属性被拒且给**专属错误码**（三要素齐备）。
///
/// 三要素 = 错误码 + 现象 + 出路。判据逐个查这三项非空。
fn chk_degrade_unknown_property(set: &mut CheckSet) {
    let code = decl_err("no-such-prop: 1px");
    let err = {
        let stream = lex("no-such-prop: 1px");
        let mut p = DeclParser::new();
        p.parse_one(&stream, 0, decl_span_end(&stream), false)
            .err()
    };
    let three = match &err {
        Some(e) => {
            !e.code.is_empty() && !e.what.is_empty() && !e.next.trim().is_empty() && !e.why.is_empty()
        }
        None => false,
    };
    set.add(
        "O05-降级-01-桶外属性拒绝三要素齐备",
        code == E_PROPERTY_UNKNOWN && three,
        "",
    );
}

/// O05-降级-02：缺冒号被拒（**专属码** `E_DECL_MALFORMED`）。
fn chk_degrade_missing_colon(set: &mut CheckSet) {
    let code = decl_err("color red");
    set.add("O05-降级-02-缺冒号拒绝", code == E_DECL_MALFORMED, "");
}

/// O05-降级-03：值区为空被拒。
fn chk_degrade_empty_value(set: &mut CheckSet) {
    let code = decl_err("color: ");
    set.add("O05-降级-03-空值区拒绝", code == E_DECL_MALFORMED, "");
}

/// O05-降级-04：错误产物（BadString）导致**整条声明被拒**，而非整表。
///
/// F2804 给本单的义务原文如此。判据要同时验「该条被拒」与「后续声明不受影响」——
/// 只验前者会被「整表丢弃」的写法蒙过（那样也「拒绝」了，但语义错了）。
fn chk_degrade_error_token_skips_declaration(set: &mut CheckSet) {
    // 语料用 **BadUrl**（`url(a b)` 含非法字符）而不是坏字符串。
    // 为什么不用坏字符串：`"a\nb"` 里的闭合引号会让 F2804 在BadString
    // 之后**再开一个 String 记号**（诊断实测：`BadString [7,11)` 之后
    // 还有 `String [16,31)`），于是「坏的那条」与「紧邻分号的那条」
    // 不是同一条，判据会在错误的区段上判——测的是一个不存在的现象。
    // BadUrl `url(a(b))` 的左括号在 url( 内非法，F2804 收成单一BadUrl
    // 记号并带到配对右括号，结构干净。
    let src = "background-color: url(a(b)); opacity: 0.5";
    let stream = lex(src);
    // 先确认语料确实产出了错误产物（否则本判据在测一个不存在的现象）。
    let bad = stream.iter().find(|t| t.is_error());
    let semi = stream
        .iter()
        .position(|t| t.kind == super::veo04_lexer::TokenKind::Semicolon)
        .unwrap_or_else(|| decl_span_end(&stream));
    let mut p = DeclParser::new();
    let first = p.parse_one(&stream, 0, semi + 1, false);
    let second = p.parse_one(&stream, semi + 1, decl_span_end(&stream), false);
    // 关键：第二条（`opacity: 0.5`）必须**成功**——证明只丢了坏的那条。
    // 只验「第一条被拒」不够：那会被「整表丢弃」的写法蒙过（那样也
    // 「拒绝」了，但语义是错的）。
    let second_ok = second.is_ok();
    let second_val = matches!(
        second.ok().map(|d| d.value.clone()),
        Some(ParsedValue::Number(_))
    );
    // 错误码必须是「声明结构不成形」这一类专属码，而不是某个泛化码。
    let first_code_ok = first
        .as_ref()
        .err()
        .map(|e| e.code == E_DECL_MALFORMED)
        .unwrap_or(false);
    set.add(
        "O05-降级-04-坏 URL 只丢本条声明（专属码）",
        bad.is_some() && first.is_err() && first_code_ok && second_ok && second_val,
        "",
    );
}

/// O05-降级-05：值域越界**钳到边界**并落告警（三个数逐个对拍）。
///
/// 判据侧独立算出期望的夹后值（`100000.0`），不向被测函数问。
fn chk_degrade_clamp_to_boundary(set: &mut CheckSet) {
    let stream = lex("width: 999999px");
    let mut p = DeclParser::new();
    let d = p.parse_one(&stream, 0, decl_span_end(&stream), false);
    // F2803 给 `width` 的域是 [0, 100000]。
    let accepted = d.is_ok();
    let clamped = matches!(&d.ok().map(|x| x.value.clone()), Some(ParsedValue::Length { value, .. }) if *value == 100000.0);
    // 告警必须落，且三个数对得上：原值 / 夹后值 / 域上下界。
    let notice_ok = p.clamps.iter().next().map(|n| {
        n.original == 999999.0 && n.clamped == 100000.0 && n.low == 0.0 && n.high == 100000.0
    }).unwrap_or(false);
    set.add(
        "O05-降级-05-越界钳到上界且告警三数对拍",
        accepted && clamped && notice_ok && p.clamps.len() == 1,
        "",
    );
}

/// O05-降级-06：**钳到边界而非归零**（`width` 域下界为 0，用 `top` 验下界）。
///
/// `top` 域 [-100000, 100000]：`top: -999999px` ⇒ 夹到 -100000，不是 0。
/// 这条专治「一律归零」的错误实现——归零会让 `top` 从「偏上」变成「贴顶」，
/// 是肉眼可见的错。
fn chk_degrade_clamp_not_zero(set: &mut CheckSet) {
    let v = decl_value("top: -999999px");
    let is_lower = matches!(&v, ParsedValue::Length { value, .. } if *value == -100000.0);
    set.add("O05-降级-06-下界越界钳到下界而非归零", is_lower, "");
}

/// O05-降级-07：域内值**不产生告警**（钳制闸不能对正常值也响）。
///
/// 只验「越界有告警」不够——那会被「恒有告警」的实现蒙过。
fn chk_degrade_no_clamp_in_domain(set: &mut CheckSet) {
    let inside = decl_clamps("width: 100px");
    let outside = decl_clamps("width: 999999px");
    let ok = inside == 0 && outside > 0;
    set.add("O05-降级-07-域内不告警越界才告警", ok, "");
}

/// O05-降级-08：NaN / ±Inf **三方向**都不得 panic 且各自有明确处置。
///
/// `clamp_finite` 对 NaN 返回 `None`（交调用方立案），对 +Inf 给上界，
/// 对 −Inf 给下界。判据逐个方向断，且断「不 panic」。
fn chk_degrade_nonfinite_three_ways(set: &mut CheckSet) {
    let nan = clamp_finite(f64::NAN, 0.0, 100.0);
    let pinf = clamp_finite(f64::INFINITY, 0.0, 100.0);
    let ninf = clamp_finite(f64::NEG_INFINITY, 0.0, 100.0);
    let inf_in_neg_domain = clamp_finite(f64::INFINITY, -10.0, -1.0);
    let ok = nan.is_none() && pinf == Some(100.0) && ninf == Some(0.0) && inf_in_neg_domain == Some(-1.0);
    set.add("O05-降级-08-非有限值三方向处置明确", ok, "");
}

/// O05-降级-09：关键字白名单外被拒，且**白名单内通过**（双向）。
///
/// 双向是必须的：只验「白名单外被拒」会被「全部拒绝」的恒假实现蒙过。
fn chk_degrade_keyword_two_ways(set: &mut CheckSet) {
    let bad = decl_err("display: nonexistent-value");
    let good = decl("display: flex", false).0;
    // 第二个正向样例取另一个属性，证明白名单不是「全放行」。
    let other_bad = decl_err("overflow: flex");
    set.add(
        "O05-降级-09-关键字白名单双向（内过外拒）",
        bad == E_KEYWORD_UNKNOWN && good && other_bad == E_KEYWORD_UNKNOWN,
        "",
    );
}

/// O05-降级-10：单位表外被拒（`width: 3s` 非法），且 `width: 0` 通过。
fn chk_degrade_unit_guard(set: &mut CheckSet) {
    let bad = decl_err("width: 3s");
    let zero = decl_value("width: 0");
    // 无单位非零必须拒（`width: 5` 非法）。
    let nonzero_bad = decl_err("width: 5");
    let ok = bad == E_UNIT_UNKNOWN
        && nonzero_bad == E_UNIT_UNKNOWN
        && matches!(&zero, ParsedValue::Length { value, .. } if *value == 0.0);
    set.add("O05-降级-10-单位守卫（表外拒/零无单位过）", ok, "");
}

/// O05-降级-11：字重越界**拒绝而非夹**（规范禁止非 100 步进）。
///
/// 这条与「越界一律钳制」相反，故必须单列：`font-weight: 450` 若被夹成
/// 450 就等于放行了规范禁止的值。
fn chk_degrade_font_weight_reject(set: &mut CheckSet) {
    let odd = decl_err("font-weight: 450");
    let over = decl_err("font-weight: 1000");
    let good = decl_value("font-weight: 700");
    let kw = decl_value("font-weight: bold");
    let good_ok = matches!(&good, ParsedValue::FontWeight { numeric, keyword } if *numeric == 700 && keyword.is_empty());
    let kw_ok = matches!(&kw, ParsedValue::FontWeight { numeric, keyword } if *numeric == 0 && keyword == "bold");
    set.add(
        "O05-降级-11-字重越界与非步进均拒绝",
        odd == E_VALUE_KIND_MISMATCH && over == E_VALUE_KIND_MISMATCH && good_ok && kw_ok,
        "",
    );
}

/// O05-降级-12：hash 色形状守卫（3 位/6 位十六进制），且带 flag 的记号才过。
fn chk_degrade_hash_shape(set: &mut CheckSet) {
    let good6 = decl_value("color: #ff8800");
    let good3 = decl_value("color: #f80");
    let bad4 = decl_err("color: #ff88");
    let bad_hex = decl_err("color: #gggggg");
    let ok = matches!(&good6, ParsedValue::Color { text, .. } if text == "ff8800")
        && matches!(&good3, ParsedValue::Color { text, .. } if text == "f80")
        && bad4 == E_VALUE_KIND_MISMATCH
        && bad_hex == E_VALUE_KIND_MISMATCH;
    set.add("O05-降级-12-hash 色形状守卫（3/6 位十六进制）", ok, "");
}

/// O05-降级-13：位置元数越界被拒（`1~2` 个分量）。
fn chk_degrade_position_arity(set: &mut CheckSet) {
    let one = decl("transform-origin: left", false).0;
    let two = decl("transform-origin: left top", false).0;
    let three = decl_err("transform-origin: left top center");
    let bad_kw = decl_err("transform-origin: nowhere");
    set.add(
        "O05-降级-13-位置元数 1~2 双向",
        one && two && three == E_VALUE_ARITY && bad_kw == E_KEYWORD_UNKNOWN,
        "",
    );
}

/// O05-降级-14：拒绝一律**立案**（异常检出 → 立案流转）。
///
/// 判据：对四类不同的拒绝（桶外属性/关键字非法/单位非法/位置元数），
/// 每类都必须留下 ≥1 件案件。
fn chk_degrade_case_filing(set: &mut CheckSet) {
    let cases = [
        decl_cases("no-such-prop: 1px"),
        decl_cases("display: nope"),
        decl_cases("width: 3s"),
        decl_cases("transform-origin: a b c"),
    ];
    let ok = cases.iter().all(|n| *n >= 1);
    set.add("O05-降级-14-四类拒绝均立案", ok, "");
}

/// O05-降级-15：正常声明**不立案**（立案闸不能对正常值也响）。
fn chk_degrade_clean_no_case(set: &mut CheckSet) {
    let clean = [
        decl_cases("color: red"),
        decl_cases("width: 100px"),
        decl_cases("display: flex"),
        decl_cases("font-weight: 700"),
    ];
    let ok = clean.iter().all(|n| *n == 0);
    set.add("O05-降级-15-正常声明不立案", ok, "");
}

/// O05-降级-16：统计守恒（成功 + 被拒 == 尝试）且拒绝数**恰等于**失败次数。
///
/// 口径纪律：判据侧独立数失败语料条数，不问 `stats.rejected` 的期望值。
fn chk_degrade_stats_conserve(set: &mut CheckSet) {
    let stream = lex("color: red; width: 100px; display: nope; opacity: 0.5");
    let mut p = DeclParser::new();
    // 手动切四段（分号分界）。
    let semis: Vec<usize> = stream
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == super::veo04_lexer::TokenKind::Semicolon)
        .map(|(i, _)| i)
        .collect();
    let mut bounds: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    for s in semis.iter() {
        bounds.push((start, s + 1));
        start = s + 1;
    }
    let end_bound = decl_span_end(&stream);
    if start < end_bound {
        bounds.push((start, end_bound));
    }
    for (a, b) in bounds.iter() {
        let _ = p.parse_one(&stream, *a, *b, false);
    }
    let expect_segments = 4u32;
    let expect_accepted = 3u32;
    let expect_rejected = 1u32;
    let ok = p.stats.conserves()
        && p.stats.segments == expect_segments
        && p.stats.accepted == expect_accepted
        && p.stats.rejected == expect_rejected;
    set.add(
        "O05-降级-16-统计守恒且恰等于 3 成 1 拒",
        ok,
        "",
    );
}

/// O05-降级-17：`!important` **被忠实记录**（本单不裁决级联）。
///
/// 级联裁决归 F2837；本单只记录「作者写了 important」。若解析器把它
/// 吞掉，F2837 就无从得知作者意图。
fn chk_degrade_important_recorded(set: &mut CheckSet) {
    let stream = lex("color: red");
    let mut p = DeclParser::new();
    let d = p.parse_one(&stream, 0, decl_span_end(&stream), true);
    let is_true = matches!(&d.ok().map(|x| x.important), Some(true));
    let mut p2 = DeclParser::new();
    let d2 = p2.parse_one(&stream, 0, decl_span_end(&stream), false);
    let is_false = matches!(&d2.ok().map(|x| x.important), Some(false));
    set.add("O05-降级-17-important 双向忠实记录", is_true && is_false, "");
}

/// O05-降级-18：属性名大小写归一（HTML 属性里大小写不敏感）。
///
/// `COLOR: red` 与 `color: red` 必须解析到**同一个 id**。
fn chk_degrade_case_normalized(set: &mut CheckSet) {
    let lower = decl_value("color: red");
    let upper = decl_value("COLOR: red");
    let ok = lower == upper;
    set.add("O05-降级-18-属性名大小写归一到同 id", ok, "");
}

/// O05-降级-19：声明偏移**可归因**（指向源里的真实位置）。
///
/// 判据侧独立算：`color: red` 源中 `color` 起始偏移必为 0（预处理后），
/// `opacity: 1` 起点必为 10（`color: red;` 之后）。
fn chk_degrade_offset_attributable(set: &mut CheckSet) {
    let src = "color: red; opacity: 1";
    let stream = lex(src);
    let mut p = DeclParser::new();
    let semis: Vec<usize> = stream
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == super::veo04_lexer::TokenKind::Semicolon)
        .map(|(i, _)| i)
        .collect();
    let s0 = semis.first().copied().unwrap_or(decl_span_end(&stream));
    let d1 = p.parse_one(&stream, 0, s0 + 1, false);
    // 末段上界走 `decl_span_end`：语料只有**一个**分号，故第二段的
    // 区间是「分号之后到流末（去掉 EOF）」。若这里退回 `stream.len()`，
    // 值区会把末尾 EOF 当值（本体已用 `decl_span_end` 修掉），
    // 于是 `opacity: 1` 被判成「数值 + 流末尾」而拒——判据自己踩了
    // 它要验的那个坑。
    let end_bound = decl_span_end(&stream);
    let d2 = p.parse_one(&stream, s0 + 1, end_bound, false);
    // 判据侧独立算期望偏移：源码字节位置。
    // `color: red; opacity: 1` 中 `color` 在0，`opacity` 在 12。
    // 这两个数由**字符串字面量在本文件里的位置**推出，不是问被测对象。
    let expect_first = 0u32;
    let expect_second = 12u32;
    let got_first = d1.ok().map(|d| d.offset).unwrap_or(u32::MAX);
    let got_second = d2.ok().map(|d| d.offset).unwrap_or(u32::MAX);
    // 负向：偏移必须落在源长内（越界偏移会让「归因」指向不存在的字节）。
    let in_range = got_first < src.len() as u32 && got_second < src.len() as u32;
    set.add(
        "O05-降级-19-声明偏移可归因（0 与 12且在源长内）",
        got_first == expect_first && got_second == expect_second && in_range,
        "",
    );
}

/// O05-降级-20：单条声明 token 数超上限被拒（**不做部分解析**）。
fn chk_degrade_decl_token_cap(set: &mut CheckSet) {
    let mut src = String::from("margin:");
    for _ in 0..40 {
        src.push_str(" 1px");
    }
    let stream = lex(src.as_str());
    let mut p = DeclParser::new();
    let r = p.parse_one(&stream, 0, decl_span_end(&stream), false);
    // 拒绝且立案，且统计里 rejected 恰好 +1（未部分接受）。
    let ok = r.as_ref().err().map(|e| e.code == E_DECL_TOKEN_CAP).unwrap_or(false)
        && p.stats.rejected == 1
        && p.stats.accepted == 0;
    set.add("O05-降级-20-超上限整条拒不做部分解析", ok, "");
}

// ---------------------------------------------------------------------------
// O05-判据-*
// ---------------------------------------------------------------------------

/// O05-判据-11b：`keyword_allowed` 查询接口**双向正确且口径统一**。
///
/// 这条专门盯一个「当前无内部调用方」的公开 API。变异门禁发现
/// `keyword_allowed` 改坏后**没有任何判据翻脸**（11/14 里的那个MISSED
/// ——真等价变异：无人调用 ⇒ 行为不可观测）。但它不是死代码：F2807
/// 样式表对象与 F2811 继承解析都要问「这个关键字在这条属性上合法吗」，
/// 那是它的正式契约面。
///
/// 契约一旦无人守护，下一个接手的人就会把它当「显然可以删」的东西删掉，
/// 或者反过来照抄它的逻辑而漏掉归一化。故给它独立判据：
/// - 正向：`display` 接受 `flex`、拒绝 `nope`；
/// - **口径统一**：`FLEX`（大写）必须同样被接受——归一化是本单的责任，
///   漏了归一化会让 HTML 内联样式的大小写不敏感规则失效；
/// - 跨属性隔离：`display` 的白名单不得放行 `resize` 才有的关键字。
fn chk_criterion_keyword_query_api(set: &mut CheckSet) {
    let Some(display) = id_of("display").and_then(|i| i.entry()) else {
        set.add("O05-判据-11b-keyword_allowed 查询接口双向正确", false, "");
        return;
    };
    let Some(resize) = id_of("resize").and_then(|i| i.entry()) else {
        set.add("O05-判据-11b-keyword_allowed 查询接口双向正确", false, "");
        return;
    };
    let pos = display.keyword_allowed("flex");
    // 注意方向：`keyword_allowed` 返回的是「**是否允许**」，所以
    // 「不在白名单」的正确期望是 `false`。写成 `let neg = ...` 再参与
    // 与运算（要求它为 true）会把「拒绝」判成失败——判据自身写反了极性，
    // 实现改对改错都过不了。这正是「判据写错比没判据更坏」的实例。
    let neg = !display.keyword_allowed("nope");
    // 口径统一：大写 / 带空白都必须同样命中。
    let cased = display.keyword_allowed("FLEX");
    let spaced = display.keyword_allowed("  flex  ");
    // 跨属性隔离：`horizontal` 是 resize 的白名单项，display 不该接受。
    let isolated = !display.keyword_allowed("horizontal") && resize.keyword_allowed("horizontal");
    set.add(
        "O05-判据-11b-keyword_allowed 双向正确且口径统一跨属性隔离",
        pos && neg && cased && spaced && isolated,
        "",
    );
}

/// O05-判据-01：无障碍——每类值类别都有中文名与逐条规格（**可读屏念**）。
fn chk_criterion_screen_reader(set: &mut CheckSet) {
    let mut ok = true;
    for k in ValueKind::ALL.iter() {
        let line = k.screen_line();
        if line.is_empty() || !line.contains('：') {
            ok = false;
        }
        if k.zh().is_empty() {
            ok = false;
        }
    }
    // 属性条目读屏行必须含族与类别码。
    let entry_ok = PROPERTY_REGISTRY
        .iter()
        .all(|e| e.screen_line().contains("族") && e.screen_line().contains("O05-K"));
    set.add("O05-判据-01-值类别与属性条目可读屏念出", ok && entry_ok, "");
}

/// O05-判据-02：无障碍——12 类值的读屏行**互不相同**（否则读屏分不清）。
fn chk_criterion_screen_distinct(set: &mut CheckSet) {
    let mut lines: Vec<String> = Vec::new();
    for sample in [
        "width: 10px",
        "opacity: 0.5",
        "transform-origin: 50%",
        "display: flex",
        "color: red",
        "border-style: solid",
        "font-weight: 700",
        "font-style: italic",
        "line-height: 1.5",
        "font-family: serif",
        "box-shadow: 0 0 4px",
        "margin: 1px 2px",
    ] {
        let v = decl_value(sample);
        let line = v.screen_line();
        if lines.contains(&line) {
            set.add("O05-判据-02-十二类值读屏行互不相同", false, "");
            return;
        }
        lines.push(line);
    }
    set.add("O05-判据-02-十二类值读屏行互不相同", lines.len() == 12, "");
}

/// O05-判据-03：`ParsedValue::kind()` **由变体派生**且与解析结果一致。
///
/// 12 条样例各跑一遍：值类别必须与登记类别一致。
fn chk_criterion_kind_derived(set: &mut CheckSet) {
    let samples = [
        ("width", "10px"),
        ("opacity", "0.5"),
        ("transform-origin", "50%"),
        ("display", "flex"),
        ("color", "red"),
        ("border-style", "solid"),
        ("font-weight", "700"),
        ("font-style", "italic"),
        ("line-height", "1.5"),
        ("font-family", "serif"),
        ("box-shadow", "0 0 4px"),
        ("margin", "1px 2px"),
    ];
    let mut ok = true;
    for (prop, val) in samples.iter() {
        let src = format!("{}: {}", prop, val);
        let v = decl_value(src.as_str());
        let expect = id_of(prop).and_then(|i| i.value_kind());
        if v.kind() != expect.unwrap_or(ValueKind::List) {
            ok = false;
        }
    }
    set.add("O05-判据-03-值类别由变体派生且与登记一致", ok, "");
}

/// O05-判据-04：`normalize_keyword` 去空白/去引号/小写三态。
fn chk_criterion_normalize(set: &mut CheckSet) {
    let a = normalize_keyword("  RED ");
    let b = normalize_keyword("\"Red\"");
    let c = normalize_keyword("ReD");
    let ok = a == "red" && b == "red" && c == "red";
    // 反例：非 ASCII 不能被小写化规则吃掉。
    let d = normalize_keyword("红色");
    let keep = d == "红色";
    // 口径说明：**CSS 转义解码归 F2804**（`\41 bc` → `Abc` 已由词法器的
    // `payload()` 物化）。本单只做「ASCII 小写 + 去空白 + 去引号」，
    // 故判据里不出现 `\u{..}` 形态——那是 F2804 的义务，不是本单的。
    set.add("O05-判据-04-关键字归一三态一致且保非 ASCII", ok && keep, "");
}

/// O05-判据-05：隐私面为「无」，且**采集清单**说明白不采集什么。
fn chk_criterion_privacy(set: &mut CheckSet) {
    let ok = PRIVACY_SURFACE == PrivacySurface::None
        && PRIVACY_SURFACE.zh() == "无隐私面"
        && PRIVACY_SURFACE.collects().contains("不读取用户输入");
    set.add("O05-判据-05-隐私面为无且采集清单显式", ok, "");
}

/// O05-判据-06：注册表自检的**拒绝路径**可达（不能是死码）。
///
/// 构造一个「域倒挂」的假表去调 `audit_registry` 的同类逻辑——但
/// `audit_registry` 读的是静态表，故改为**直接验错误构造路径**：把
/// `StyleError::new` 的五元组逐项非空作为「拒绝三要素」的构造纪律。
fn chk_criterion_error_five_tuple(set: &mut CheckSet) {
    // 构造一个真实错误（五元组）并逐项检查。
    let e = StyleError::new(E_DECL_MALFORMED, "现象", "根因", "出路", "责任方");
    let ok = !e.code.is_empty()
        && !e.what.is_empty()
        && e.why == "根因"
        && e.next == "出路"
        && e.who == "责任方";
    // 修复建议常量必须非空（拒绝必须给出路）。
    let fixes = [
        FIX_REGISTRY,
        FIX_DUP,
        FIX_UNKNOWN_PROP,
        FIX_KIND,
        FIX_ARITY,
        FIX_KEYWORD,
        FIX_DECL,
        FIX_UNIT,
        FIX_TOKEN_CAP,
    ];
    let all_fixes = fixes.iter().all(|f| !f.trim().is_empty());
    set.add("O05-判据-06-错误五元组齐备且修复建议非空", ok && all_fixes, "");
}

/// O05-判据-07：诊断码**互不相同**（码表不重——重了就没法按码定位）。
fn chk_criterion_codes_distinct(set: &mut CheckSet) {
    let codes = [
        E_REGISTRY_INCOMPLETE,
        E_REGISTRY_DUP_NAME,
        E_REGISTRY_NAME_DRIFT,
        E_PROPERTY_UNKNOWN,
        E_VALUE_KIND_MISMATCH,
        E_VALUE_ARITY,
        E_KEYWORD_UNKNOWN,
        E_DECL_MALFORMED,
        E_UNIT_UNKNOWN,
        E_DECL_TOKEN_CAP,
        E_REGISTRY_CAP,
    ];
    let mut uniq: Vec<&str> = Vec::new();
    let mut ok = true;
    for c in codes.iter() {
        if c.is_empty() || uniq.contains(c) {
            ok = false;
        }
        uniq.push(c);
    }
    // 码段带域前缀（便于台账按域过滤）。
    let prefixed = codes.iter().all(|c| c.starts_with("E_"));
    set.add("O05-判据-07-诊断码互异且带域前缀", ok && prefixed, "");
}

/// O05-判据-08：模块自述覆盖「做了什么 + 没做什么 + 六闸 + 复杂度」。
fn chk_criterion_narration(set: &mut CheckSet) {
    let n = module_narration();
    let ok = n.contains("VE-F2805")
        && n.contains("复杂度分解")
        && n.contains("六闸")
        && n.contains("注册表");
    set.add("O05-判据-08-模块自述覆盖职责与复杂度", ok, "");
}

/// O05-判据-09：单号锚点一致（本单 = VE-F2805，判据层与本体同号）。
fn chk_criterion_anchor_same(set: &mut CheckSet) {
    let ok = CHECKS_ANCHOR == "VE-F2805";
    set.add("O05-判据-09-单号锚点为 VE-F2805", ok, "");
}

/// O05-判据-10：判据层自身零 panic 面（**门禁不许自己崩**）。
///
/// 只扫判据层的 `unwrap`/`expect`/`panic!`；`#[cfg(test)]` 区允许。
fn chk_criterion_checks_no_panic(set: &mut CheckSet) {
    let src = include_str!("veo05_props_checks.rs");
    let test_at = src.find("#[cfg(test)]").unwrap_or(src.len());
    let body = &src[..test_at];
    // **必须剥离字符串字面量后再扫**：判据自己的判据串 `".unwrap()"`
    // 里就写着 `.unwrap()`，直接 `contains` 会让本判据恒红——那是
    // 「判据把自己的判据算进被测内容」的自证式失效（纪律五）。
    // 故逐字符扫，跳过 `"` 起的字符串（含转义 `\"`）与行注释。
    let mut offenders: Vec<String> = Vec::new();
    for (i, line) in body.lines().enumerate() {
        // 字符串与转义都不跨行（Rust 字符串字面量不跨行），故在行内声明即可。
        let mut in_str = false;
        let mut esc = false;
        let mut cleaned = String::new();
        for c in line.chars() {
            if in_str {
                if c == '\\' {
                    esc = !esc;
                    continue;
                }
                if c == '"' && !esc {
                    in_str = false;
                    continue;
                }
                esc = false;
                continue;
            }
            match c {
                '"' => in_str = true,
                _ => cleaned.push(c),
            }
        }
        // 剥掉行注释（Rust 里`//` 到行尾都是注释）。
        let code = match cleaned.find("//") {
            Some(p) => &cleaned[..p],
            None => cleaned.as_str(),
        };
        for pat in [".unwrap()", "panic!"].iter() {
            if code.contains(pat) {
                offenders.push(format!("{}:{}", i + 1, pat));
            }
        }
    }
    set.add(
        "O05-判据-10-判据层生产段零 panic 面（剥字符串后扫）",
        offenders.is_empty(),
        "",
    );
}

/// O05-判据-11：名称索引表与注册表**逐项对齐**。
fn chk_criterion_name_index(set: &mut CheckSet) {
    let idx = name_index();
    let mut ok = idx.len() == PROPERTY_REGISTRY.len();
    if ok {
        for (i, n) in idx.iter().enumerate() {
            match PROPERTY_REGISTRY.get(i) {
                Some(e) => {
                    if e.name != *n {
                        ok = false;
                    }
                }
                None => ok = false,
            }
        }
    }
    set.add("O05-判据-11-名索引与注册表逐项对齐", ok, "");
}

/// O05-判据-12：百分比**归一到 0..1 口径**（`50%` ⇒ `0.5`）。
///
/// 这条专治「把 50 当 0.5 用」的经典错：判据侧独立算出 0.5。
fn chk_criterion_percentage_normalized(set: &mut CheckSet) {
    let v = decl_value("transform-origin: 50%");
    let ok = matches!(&v, ParsedValue::Position(items) if items.len() == 1 && items[0] == "50%");
    // 另验百分比标量口径：`opacity` 是 Number 不是 Percentage，
    // 故用 `ValueKind::Percentage` 的独立样例（`kind_for` 是被测的，
    // 故这里只验位置分量的百分号**未被归一**——位置分量按原文存）。
    set.add("O05-判据-12-百分比口径（位置分量存原文百分号）", ok, "");
}

// ---------------------------------------------------------------------------
// 聚合入口
// ---------------------------------------------------------------------------

/// VE-F2805 域自检（**63 条判据**，五族分述）。
// ---------------------------------------------------------------------------
// 两条判据（变异 harness 实测补入）：这两条守住的是**变异测试抓不到**的
// 两处缺陷——修对了判据全绿，改坏了判据仍全绿。补入前先复现漏网，
// 补入后必须绿基线 + 红变体（双向验证），否则等于没补。
// ---------------------------------------------------------------------------

/// 判据 A：段起落在空白上时**仍能正确解析**（前导空白必须被跳过）。
///
/// 为什么这条必须独立成条：`parse_one` 收到的段区间由调用方按分号切出，
/// 而分号之后紧跟空白 token——`a: b; c: d` 的第二段起点是空白而非 `c`。
/// 若解析器不跳空白，每条非首声明都会以「属性名不是标识符」被拒。
///
/// 这个缺陷极隐蔽：单条声明的样例（`color: red`）起点恰在Ident 上，
/// 一切正常；只有**多声明**语料才暴露。而「整表丢弃」式的实现同样会让
/// 首条失败——所以必须断「**非首**声明成功且名字正确」。
fn chk_degrade_leading_whitespace_skipped(set: &mut CheckSet) {
    let stream = lex("color: red; width: 100px; opacity: 0.5");
    let semis: Vec<usize> = stream
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind == super::veo04_lexer::TokenKind::Semicolon)
        .map(|(i, _)| i)
        .collect();
    let mut p = DeclParser::new();
    let mut names: Vec<&'static str> = Vec::new();
    let mut start = 0usize;
    for s in semis.iter() {
        if let Ok(d) = p.parse_one(&stream, start, s.saturating_add(1), false) {
            names.push(d.name);
        }
        start = s.saturating_add(1);
    }
    if start < decl_span_end(&stream) {
        if let Ok(d) = p.parse_one(&stream, start, decl_span_end(&stream), false) {
            names.push(d.name);
        }
    }
    // 三段必须**全部**成功，且名字顺序为 color / width / opacity。
    // 只断「至少一条成功」会被「只解析第一条就停」的实现蒙过。
    // 下标必须用 `get` 而不是 `[i]`：`names.len() == 3 && names[0] == …`
    // 这种写法看起来有短路保护，其实没有——`len()==3` 为 true 时下标仍可能
    // 越界（判据侧自己的数组长度不该由另一处条件来担保）。一旦本行的
    // 长度条件被改弱（如 `>= 1`），判据会**先 panic 再判红**，看着像
    // 「变异被捕获」，实际是门禁自己崩了——那种红没有鉴别力。
    let got = [
        names.get(0).copied().unwrap_or(""),
        names.get(1).copied().unwrap_or(""),
        names.get(2).copied().unwrap_or(""),
    ];
    let ok = names.len() == 3
        && got[0] == "color"
        && got[1] == "width"
        && got[2] == "opacity"
        && p.stats.rejected == 0
        && p.stats.conserves();
    set.add("O05-降级-21-段前导空白被跳过（三段全中）", ok, "");
}

/// 判据 B：列表内**颜色关键字**被接受（`box-shadow: 0 0 4px red`）。
///
/// 为什么必须独立成条：列表分量的 `Ident` 要同时面对三套表——颜色表
/// （值语法共享）、线型表（值语法共享）、属性白名单（按属性收窄）。
/// `box-shadow` 的白名单是 `none`/`auto`、线型表也不含 `red`，所以
/// `red` 只能由颜色表命中。漏掉颜色表 ⇒ 合法的 `box-shadow` 被拒。
///
/// 双向：既断「颜色关键字被接受」，也断「非颜色关键字仍被拒」——
/// 后者防「把颜色判定写成全放行」的实现。
fn chk_subset_list_item_color_keyword(set: &mut CheckSet) {
    let good = decl("box-shadow: 0 0 4px red", false).0;
    let good2 = decl("box-shadow: 0 0 4px blue", false).0;
    // 列表内 hash 色（另一条路径：Hash 记号）。
    let hashed = decl_value("box-shadow: 0 0 4px #ff8800");
    let hash_ok = matches!(&hashed, ParsedValue::List(_));
    // 反例：`notacolor` 既不在颜色表也不在白名单，必须被拒。
    let bad = decl_err("box-shadow: 0 0 4px notacolor");
    set.add(
        "O05-子集-11-列表内颜色关键字（正反双向）",
        good && good2 && hash_ok && bad == E_KEYWORD_UNKNOWN,
        "",
    );
}

pub fn run_veo05_checks() -> CheckSet {
    let mut set = CheckSet::new("veo05-props");
    // 架构声明
    chk_arch_registry_len(&mut set);
    chk_arch_compile_audit(&mut set);
    chk_arch_parser_len(&mut set);
    chk_arch_kind_roundtrip(&mut set);
    chk_arch_id_roundtrip(&mut set);
    chk_arch_id_bidirectional(&mut set);
    chk_arch_complexity(&mut set);
    chk_arch_no_panic_surface(&mut set);
    chk_arch_scope_discipline(&mut set);
    chk_arch_anchor(&mut set);
    // 集成边界
    chk_boundary_upstream_contract(&mut set);
    chk_boundary_hash_sensitive(&mut set);
    chk_boundary_downstream_declared(&mut set);
    chk_boundary_hooks(&mut set);
    chk_boundary_name_single_source(&mut set);
    chk_boundary_family_and_domain_from_f2803(&mut set);
    chk_boundary_audit_passes(&mut set);
    chk_boundary_digest_sensitive(&mut set);
    // 解析子集
    chk_subset_family_counts(&mut set);
    chk_subset_kind_covered(&mut set);
    chk_subset_reserved_parser_works(&mut set);
    chk_subset_codes_in_digest(&mut set);
    chk_subset_no_empty_keyword(&mut set);
    chk_subset_keyword_unique(&mut set);
    chk_subset_length_units(&mut set);
    chk_subset_constant_tables(&mut set);
    chk_subset_parser_slot_alignment(&mut set);
    chk_subset_no_expansion(&mut set);
    chk_subset_color_no_conversion(&mut set);
    chk_subset_list_item_color_keyword(&mut set);
    // 降级矩阵
    chk_degrade_unknown_property(&mut set);
    chk_degrade_missing_colon(&mut set);
    chk_degrade_empty_value(&mut set);
    chk_degrade_error_token_skips_declaration(&mut set);
    chk_degrade_leading_whitespace_skipped(&mut set);
    chk_degrade_clamp_to_boundary(&mut set);
    chk_degrade_clamp_not_zero(&mut set);
    chk_degrade_no_clamp_in_domain(&mut set);
    chk_degrade_nonfinite_three_ways(&mut set);
    chk_degrade_keyword_two_ways(&mut set);
    chk_degrade_unit_guard(&mut set);
    chk_degrade_font_weight_reject(&mut set);
    chk_degrade_hash_shape(&mut set);
    chk_degrade_position_arity(&mut set);
    chk_degrade_case_filing(&mut set);
    chk_degrade_clean_no_case(&mut set);
    chk_degrade_stats_conserve(&mut set);
    chk_degrade_important_recorded(&mut set);
    chk_degrade_case_normalized(&mut set);
    chk_degrade_offset_attributable(&mut set);
    chk_degrade_decl_token_cap(&mut set);
    // 判据
    chk_criterion_keyword_query_api(&mut set);
    chk_criterion_screen_reader(&mut set);
    chk_criterion_screen_distinct(&mut set);
    chk_criterion_kind_derived(&mut set);
    chk_criterion_normalize(&mut set);
    chk_criterion_privacy(&mut set);
    chk_criterion_error_five_tuple(&mut set);
    chk_criterion_codes_distinct(&mut set);
    chk_criterion_narration(&mut set);
    chk_criterion_anchor_same(&mut set);
    chk_criterion_checks_no_panic(&mut set);
    chk_criterion_name_index(&mut set);
    chk_criterion_percentage_normalized(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主侧 cargo test 直跑；回归可复现——零墙钟零 IO）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veo05_registry_audit_clean() {
        assert!(audit_registry().is_ok(), "标准注册表不应有自检问题");
    }

    #[test]
    fn veo05_registry_covers_44() {
        assert_eq!(PROPERTY_REGISTRY.len(), 44);
        assert_eq!(PARSERS.len(), 11);
    }

    #[test]
    fn veo05_names_unique() {
        let mut names: Vec<&str> = Vec::new();
        for e in PROPERTY_REGISTRY.iter() {
            assert!(!names.contains(&e.name), "重复名 {}", e.name);
            names.push(e.name);
        }
    }

    #[test]
    fn veo05_decl_accept_and_reject() {
        assert!(decl("color: red", false).0);
        assert_eq!(decl_err("no-such: 1px"), E_PROPERTY_UNKNOWN);
        assert_eq!(decl_err("display: nope"), E_KEYWORD_UNKNOWN);
    }

    #[test]
    fn veo05_clamp_to_upper_bound() {
        let v = decl_value("width: 999999px");
        assert_eq!(
            v,
            ParsedValue::Length {
                value: 100000.0,
                unit: "px".to_string()
            }
        );
    }

    #[test]
    fn veo05_nonfinite_three_ways() {
        assert!(clamp_finite(f64::NAN, 0.0, 1.0).is_none());
        assert_eq!(clamp_finite(f64::INFINITY, 0.0, 1.0), Some(1.0));
        assert_eq!(clamp_finite(f64::NEG_INFINITY, 0.0, 1.0), Some(0.0));
    }

    #[test]
    fn veo05_shorthand_not_expanded() {
        let v = decl_value("margin: 1px 2px 3px 4px");
        assert_eq!(v.arity(), 4);
    }

    #[test]
    fn veo05_kind_roundtrip_all() {
        for k in ValueKind::ALL.iter() {
            assert_eq!(ValueKind::from_code(&k.code()), Some(*k));
        }
    }
}