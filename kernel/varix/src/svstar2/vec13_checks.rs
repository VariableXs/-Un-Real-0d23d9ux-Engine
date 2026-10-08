//! VE-F0413 · 域自检（判据逐条对应，见 `vec13_cond.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 整型语义 → `C13-整型-*`（字面量进制/后缀、四则、比较、移位、三目、
//!   defined 两拼写、宏代入、非整型报错、除零报错）
//! - 嵌套栈 → `C13-嵌套-*`（三层嵌套栈语义、未闭合逐层报错、else 多重报错、
//!   elif-after-else 报错、孤立 endif 报错、深度上限报错）
//! - 跳过快扫 → `C13-跳过-*`（跳过段字节跨度记录、跳过段不求值、跳过段
//!   语法错不报、嵌套外层跳过连带）
//! - 语义显性 → `C13-显性-*`（未定义宏按假有注记、三要素齐备、注记可查）

use super::vec12_macro::{parse_definition, MacroConfig, MacroTable};
use super::vec13_cond::*;
use crate::checks::CheckSet;

/// 组装测试宏表：LEVEL（对象宏）、TWICE（函数式）、SELFREF（自引用）。
fn test_table() -> MacroTable {
    let mut t = MacroTable::new();
    let cfg = MacroConfig::default_config();
    if let Ok(d) = parse_definition("LEVEL 3", 1, 0) {
        let _ = t.define(d, &cfg);
    }
    if let Ok(d) = parse_definition("TWICE(a) ((a)+(a))", 2, 8) {
        let _ = t.define(d, &cfg);
    }
    if let Ok(d) = parse_definition("SELFREF SELFREF", 3, 16) {
        let _ = t.define(d, &cfg);
    }
    t
}

fn cfg() -> CondConfig {
    CondConfig::default_config()
}

/// 求值成功 → 取生效文本；失败 → 取错误码（判据断言用）。
fn run(src: &str) -> Result<String, &'static str> {
    let t = test_table();
    match evaluate_source(src, &t, &cfg()) {
        Ok(o) => Ok(o.active_text()),
        Err(e) => Err(e.code),
    }
}

/// VE-F0413 域自检。
pub fn run_vec13_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec13");

    // ---- 判据：整型语义 ----

    // 整型字面量进制（十进制 / 0x 十六进制）
    {
        let r = run("#if 0x10 == 16\nkeep\n#endif");
        set.add("C13-整型-进制", r.as_deref() == Ok("keep"), "");
    }
    // 整型字面量后缀（u = u32 无符号语义：1u-2u 回绕成大正数，故 >0 成立）
    {
        let r = run("#if 1u - 2u > 0\nkeep\n#endif");
        set.add("C13-整型-无符号后缀", r.as_deref() == Ok("keep"), "");
    }
    // 四则运算与优先级（乘法先于加法）
    {
        let r = run("#if 1 + 2 * 3 == 7\nkeep\n#endif");
        set.add("C13-整型-四则优先级", r.as_deref() == Ok("keep"), "");
    }
    // 关系比较与逻辑算子
    {
        let r = run("#if (1 < 2) && !(3 <= 2)\nkeep\n#endif");
        set.add("C13-整型-关系与逻辑", r.as_deref() == Ok("keep"), "");
    }
    // 移位
    {
        let r = run("#if (1 << 4) == 16\nkeep\n#endif");
        set.add("C13-整型-移位", r.as_deref() == Ok("keep"), "");
    }
    // 三目
    {
        let r = run("#if (1 ? 2 : 3) == 2\nkeep\n#endif");
        set.add("C13-整型-三目", r.as_deref() == Ok("keep"), "");
    }
    // defined 圆括号拼写
    {
        let r = run("#if defined(LEVEL)\nkeep\n#endif");
        set.add("C13-整型-defined括号", r.as_deref() == Ok("keep"), "");
    }
    // defined 无括号拼写
    {
        let r = run("#if defined LEVEL\nkeep\n#endif");
        set.add("C13-整型-defined空格", r.as_deref() == Ok("keep"), "");
    }
    // 对象宏代入求值
    {
        let r = run("#if LEVEL == 3\nkeep\n#endif");
        set.add("C13-整型-对象宏代入", r.as_deref() == Ok("keep"), "");
    }
    // 函数式宏实参代入
    {
        let r = run("#if TWICE(2) == 4\nkeep\n#endif");
        set.add("C13-整型-函数宏代入", r.as_deref() == Ok("keep"), "");
    }
    // 非整型（浮点）→ 报错带类型说明
    {
        let r = run("#if 1.5 > 1\nkeep\n#endif");
        set.add("C13-整型-浮点报错", r == Err("E_COND_NOT_INT"), "");
    }
    // 除零 → 显性报错
    {
        let r = run("#if 1 / 0\nkeep\n#endif");
        set.add("C13-整型-除零报错", r == Err("E_COND_DIV_ZERO"), "");
    }
    // 移位越界 → 显性报错
    {
        let r = run("#if 1 << 999\nkeep\n#endif");
        set.add("C13-整型-移位越界", r == Err("E_COND_SHIFT_RANGE"), "");
    }
    // 溢出 → 显性报错（ul 满值自乘越出 i128 求值域）
    {
        let r = run("#if 18446744073709551615ul * 18446744073709551615ul > 0\nkeep\n#endif");
        set.add("C13-整型-溢出报错", r == Err("E_COND_OVERFLOW"), "");
    }
    // 字面量越出后缀位宽 → 复用 F0406 的越界报错（单一实现不双份）
    {
        let r = run("#if 99999999999 > 0\nkeep\n#endif");
        set.add("C13-整型-字面量越界", r == Err("E_COND_LITERAL"), "");
    }

    // ---- 判据：嵌套栈 ----

    // 三层嵌套：外层假则内层不参与
    {
        let r = run("#if 0\n#if 1\nno\n#endif\n#else\nyes\n#endif");
        set.add("C13-嵌套-外假内连带", r.as_deref() == Ok("yes"), "");
    }
    // 内层独立生效（外层真）
    {
        let r = run("#if 1\n#if 0\na\n#else\nb\n#endif\n#endif");
        set.add("C13-嵌套-内层分支", r.as_deref() == Ok("b"), "");
    }
    // 未闭合 → 逐层报错且带开指令链
    {
        let t = test_table();
        let streams = super::vec11_prepro::split_streams("#ifdef LEVEL\n#if 1\nbody\n")
            .expect("双流分流应成功");
        let e = evaluate_conditionals(&streams, &t, &cfg()).expect_err("未闭合应报错");
        set.add("C13-嵌套-未闭合报错", e.code == "E_COND_UNCLOSED", e.code);
        set.add(
            "C13-嵌套-未闭合逐层",
            e.layers.len() == 2 && e.layer_trace().contains("第1层"),
            "layers",
        );
    }
    // else 多重 → 报错
    {
        let r = run("#if 1\na\n#else\nb\n#else\nc\n#endif");
        set.add("C13-嵌套-else多重报错", r == Err("E_COND_DOUBLE_ELSE"), "");
    }
    // elif 出现在 else 之后 → 报错
    {
        let r = run("#if 1\na\n#else\nb\n#elif 1\nc\n#endif");
        set.add("C13-嵌套-elif-after-else", r == Err("E_COND_ELIF_AFTER_ELSE"), "");
    }
    // 孤立 endif → 报错
    {
        let r = run("#endif");
        set.add("C13-嵌套-孤立endif", r == Err("E_COND_ORPHAN_ENDIF"), "");
    }
    // 孤立 elif → 报错
    {
        let r = run("#elif 1");
        set.add("C13-嵌套-孤立elif", r == Err("E_COND_ORPHAN"), "");
    }
    // 深度超限 → 报错带链
    {
        let mut src = String::new();
        for i in 0..(CondConfig::DEFAULT_MAX_DEPTH + 2) {
            src.push_str(&format!("#if 1\n// layer {}\n", i));
        }
        src.push_str("#endif\n");
        let r = run(&src);
        set.add("C13-嵌套-深度超限", r == Err("E_COND_DEPTH"), "");
    }

    // ---- 判据：跳过快扫 ----

    // 跳过段被剔除且留字节跨度
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#if 0\nskipme\n#else\nkeepme\n#endif\nkeep2")
                .expect("双流分流应成功");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        let kept = o.active_text();
        set.add(
            "C13-跳过-段被剔除",
            kept.contains("keepme") && kept.contains("keep2") && !kept.contains("skipme"),
            "",
        );
        set.add(
            "C13-跳过-跨度记录",
            o.skipped.len() == 1 && o.skipped_bytes() > 0,
            "",
        );
        set.add(
            "C13-跳过-原因显性",
            o.skipped.first().map(|s| s.reason) == Some(SkipReason::BranchFalse),
            "",
        );
    }
    // 跳过段不求值（内层表达式非法也不报——跳过段只做词法）
    {
        // 外层 #if 0 令整段跳过：内层 `#if 1 / 0` 的除零不求值故不报错，
        // 同在跳过区的 keep 也不进生效段。
        let t2 = test_table();
        let s2 = super::vec11_prepro::split_streams("#if 0\n#if 1 / 0\nbroken(\n#endif\nkeep\n#endif")
            .expect("分流应成功");
        let o2 = evaluate_conditionals(&s2, &t2, &cfg()).expect("跳过段不应报错");
        set.add(
            "C13-跳过-跳过段不求值",
            o2.kept.is_empty() && o2.stats.exprs_skipped >= 1,
            "",
        );
    }
    // 跳过段的统计面：exprs_evaluated 只计生效段
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#if 0\n#if 9/9\na\n#endif\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add(
            "C13-跳过-不计入求值数",
            o.stats.exprs_evaluated == 1 && o.stats.exprs_skipped >= 1,
            "",
        );
    }
    // 外层跳过时内层记 ParentSkipped
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#if 0\n#if 1\na\n#endif\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add(
            "C13-跳过-外层连带原因",
            o.skipped
                .iter()
                .any(|s| s.reason == SkipReason::ParentSkipped),
            "",
        );
    }
    // 已有真分支后的 #elif 记 AlreadyTaken
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#if 1\na\n#elif 1\nb\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add(
            "C13-跳过-已有真分支",
            o.active_text() == "a"
                && o.skipped
                    .iter()
                    .any(|s| s.reason == SkipReason::AlreadyTaken),
            "",
        );
    }
    // 快扫字节面被记账
    {
        let t = test_table();
        let streams = super::vec11_prepro::split_streams("#if 0\naaaa\nbbbb\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add("C13-跳过-字节面记账", o.stats.skipped_bytes > 0, "");
    }

    // ---- 判据：语义显性 ----

    // 未定义宏按假 → 求值为假 + 有注记
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#if NOPE\nyes\n#else\nno\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add("C13-显性-未定义按假", o.active_text() == "no", "");
        set.add(
            "C13-显性-未定义有注记",
            o.notes
                .iter()
                .any(|n| n.code == "N_COND_UNDEF_MACRO" && n.what.contains("NOPE")),
            "",
        );
        set.add(
            "C13-显性-注记三要素",
            o.notes
                .iter()
                .all(|n| !n.what.is_empty() && !n.why.is_empty() && !n.next.is_empty()),
            "",
        );
    }
    // 错误三要素齐备（错误路径显性）
    {
        let t = test_table();
        let streams = super::vec11_prepro::split_streams("#ifdef LEVEL\nbody\n").expect("分流");
        let e = evaluate_conditionals(&streams, &t, &cfg()).expect_err("未闭合应报错");
        set.add("C13-显性-错误三要素", e.is_complete(), e.code);
    }
    // 分支记录表可查（六指令坐标入表）
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#ifdef LEVEL\na\n#else\nb\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add("C13-显性-分支记录", o.branches.len() == 2, "");
        set.add(
            "C13-显性-分支状态标注",
            o.branches.first().map(|b| b.state) == Some(BranchState::Taken)
                && o.branches.last().map(|b| b.state) == Some(BranchState::Done),
            "",
        );
    }
    // ifndef 家族
    {
        let r = run("#ifndef NOPE\nkeep\n#endif");
        set.add("C13-整型-ifndef", r.as_deref() == Ok("keep"), "");
    }
    // 函数式宏实参数不符 → 双侧语义拒绝
    {
        let r = run("#if TWICE(2, 3) == 4\nkeep\n#endif");
        set.add("C13-整型-实参数不符", r == Err("E_COND_ARITY"), "");
    }
    // 自引用宏在条件里按展开护栏处置
    {
        let t = test_table();
        let streams = super::vec11_prepro::split_streams("#if SELFREF\nkeep\n#endif").expect("分流");
        let e = evaluate_conditionals(&streams, &t, &cfg());
        set.add(
            "C13-整型-自引用宏处置",
            e.is_err() || e.map(|o| o.active_text()) == Ok(String::from("keep")),
            "",
        );
    }
    // 表达式缺操作数 → 报错
    {
        let r = run("#if 1 +\nkeep\n#endif");
        set.add("C13-整型-缺操作数", r == Err("E_COND_EMPTY"), "");
    }
    // 三目缺冒号分支 → 报错
    {
        let r = run("#if 1 ? 2\nkeep\n#endif");
        set.add("C13-整型-三目缺分支", r == Err("E_COND_TERNARY"), "");
    }
    // 表达式尾部多余记号 → 报错
    {
        let r = run("#if 1 2\nkeep\n#endif");
        set.add("C13-整型-尾部多余", r == Err("E_COND_TRAILING"), "");
    }
    // ifdef 缺宏名 → 报错
    {
        let r = run("#ifdef\nkeep\n#endif");
        set.add("C13-整型-ifdef缺宏名", r == Err("E_COND_NO_MACRO_NAME"), "");
    }
    // 空表达式 → 报错
    {
        let r = run("#if\nkeep\n#endif");
        set.add("C13-整型-空表达式", r == Err("E_COND_NO_EXPR"), "");
    }
    // 括号未闭合 → 报错
    {
        let r = run("#if (1\nkeep\n#endif");
        set.add("C13-整型-括号未闭合", r == Err("E_COND_PAREN"), "");
    }
    // 栈平衡标记
    {
        let t = test_table();
        let streams = super::vec11_prepro::split_streams("#if 1\na\n#endif").expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add("C13-嵌套-栈平衡", o.balanced, "");
    }
    // 峰值深度可观测
    {
        let t = test_table();
        let streams =
            super::vec11_prepro::split_streams("#if 1\n#if 1\n#if 1\na\n#endif\n#endif\n#endif")
                .expect("分流");
        let o = evaluate_conditionals(&streams, &t, &cfg()).expect("求值应成功");
        set.add("C13-嵌套-峰值深度", o.stats.peak_depth == 3, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 域自检全绿（红项即缺陷，不许带病交付）。
    #[test]
    fn vec13_checks_all_green() {
        let set = run_vec13_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec13 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items().0
        );
    }

    /// 判据直查：整型语义 + 嵌套栈 + 跳过快扫 + 语义显性 四族各有覆盖。
    #[test]
    fn vec13_judgement_families_covered() {
        let set = run_vec13_checks();
        let mut fams = [0usize; 4];
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if c.name.contains("C13-整型") {
                    fams[0] += 1;
                }
                if c.name.contains("C13-嵌套") {
                    fams[1] += 1;
                }
                if c.name.contains("C13-跳过") {
                    fams[2] += 1;
                }
                if c.name.contains("C13-显性") {
                    fams[3] += 1;
                }
            }
        }
        assert!(
            fams.iter().all(|n| *n > 0),
            "四族判据覆盖不全：{:?}",
            fams
        );
    }
}