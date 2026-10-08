//! VE-F0412 · 域自检（判据逐条对应，见 `vec12_macro.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 记号级展开 → `C12-展开-*`（对象宏/函数式/参数代入/#串接/##粘贴）
//! - 递归冻结 → `C12-冻结-*`（自引用原样保留、深度超限指向定义）
//! - 双侧定位 → `C12-定位-*`（参数个数不符给定义处+调用处）
//! - 深度上限 → `C12-深度-*`（互递归截断、上限可配）

use super::vec12_macro::*;
use crate::checks::CheckSet;

/// 组装测试宏表：PI=对象宏、ADD=函数式、SELF=自引用、MUTA/MUTB=互引用。
fn test_table() -> MacroTable {
    let mut t = MacroTable::new();
    let cfg = MacroConfig::default_config();
    let d1 = parse_definition("PI 314", 1, 0).unwrap();
    let d2 = parse_definition("ADD(a,b) ((a)+(b))", 2, 5).unwrap();
    let d3 = parse_definition("SELF SELF", 3, 9).unwrap();
    let d4 = parse_definition("MUTA ADD(MUTB,1)", 4, 12).unwrap();
    let d5 = parse_definition("MUTB MUTA", 5, 15).unwrap();
    let _ = t.define(d1, &cfg);
    let _ = t.define(d2, &cfg);
    let _ = t.define(d3, &cfg);
    let _ = t.define(d4, &cfg);
    let _ = t.define(d5, &cfg);
    t
}

/// VE-F0412 域自检。
pub fn run_vec12_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec12");
    let cfg = MacroConfig::default_config();

    // ---- 判据：记号级展开 ----

    // 对象宏展开
    {
        let t = test_table();
        let r = expand_input(&t, "x = PI;", &cfg).unwrap();
        set.add(
            "C12-展开-对象宏",
            r.text == "x = 314 ;",
            "",
        );
    }
    // 函数式宏参数代入
    {
        let t = test_table();
        let r = expand_input(&t, "ADD(1,2)", &cfg).unwrap();
        set.add(
            "C12-展开-函数式参数代入",
            r.text == "( ( 1 ) + ( 2 ) )",
            "",
        );
    }
    // 定义表解析：对象宏 vs 函数式宏（紧贴括号才算参数表）
    {
        let a = parse_definition("MAX 100", 1, 0).unwrap();
        let b = parse_definition("FN(x) x+1", 2, 3).unwrap();
        let c = parse_definition("OBJ (x) x+1", 3, 6).unwrap();
        set.add(
            "C12-展开-定义解析",
            a.params.is_none() && a.body == "100"
                && b.params.as_deref() == Some(&["x".to_string()]) && b.body == "x+1"
                && c.params.is_none() && c.body == "(x) x+1",
            "",
        );
    }
    // # 串接：#参数 → 实参实文的字符串字面量
    {
        let mut t = MacroTable::new();
        let d = parse_definition("STR(x) #x", 1, 0).unwrap();
        let _ = t.define(d, &cfg);
        let r = expand_input(&t, "STR(hello world)", &cfg).unwrap();
        set.add(
            "C12-展开-井号串接",
            r.text.contains("\"hello world\""),
            "",
        );
    }
    // ## 粘贴：左##右拼成单记号（实参取实文）
    {
        let mut t = MacroTable::new();
        let d = parse_definition("CAT(a,b) a##b", 1, 0).unwrap();
        let _ = t.define(d, &cfg);
        let r = expand_input(&t, "CAT(foo, bar)", &cfg).unwrap();
        set.add(
            "C12-展开-井井粘贴",
            r.text == "foobar",
            "",
        );
    }
    // 嵌套实参先展开再代入（实参里的宏先展开）
    {
        let t = test_table();
        let r = expand_input(&t, "ADD(PI,2)", &cfg).unwrap();
        set.add(
            "C12-展开-实参先展开",
            r.text == "( ( 314 ) + ( 2 ) )",
            "",
        );
    }

    // ---- 判据：递归冻结 ----

    // 自引用宏按原样保留（蓝漆），不死循环
    {
        let t = test_table();
        let r = expand_input(&t, "SELF", &cfg).unwrap();
        set.add(
            "C12-冻结-自引用原样保留",
            r.text == "SELF",
            "",
        );
    }

    // ---- 判据：深度上限 ----

    // 互引用对被冻结语义直接拦下（MUTA→MUTB→MUTA 冻结原样保留），不进深度计
    {
        let t = test_table();
        let r = expand_input(&t, "MUTA", &cfg).unwrap();
        set.add(
            "C12-冻结-互引用拦下",
            r.text.contains("MUTA"),
            "",
        );
    }
    // 深度上限：互异宏链（M0→M1→…→M69）超过小上限时报错指向定义
    {
        let mut t = MacroTable::new();
        for k in 0..50u32 {
            let body = if k == 49 {
                alloc::format!("M{} 0", k)
            } else {
                alloc::format!("M{} M{}", k, k + 1)
            };
            let d = parse_definition(&alloc::format!("M{} {}", k, body), k as usize, k as usize).unwrap();
            let _ = t.define(d, &cfg);
        }
        let small = MacroConfig { max_depth: 4, redef: RedefPolicy::Warn };
        let e = expand_input(&t, "M0", &small).unwrap_err();
        set.add(
            "C12-深度-互异链截断指向定义",
            e.code == "E_MACRO_DEPTH" && e.def_pos.is_some() && e.is_complete(),
            "",
        );
        // 上限放宽后同链合法收敛
        let r = expand_input(&t, "M0", &cfg);
        set.add("C12-深度-上限放宽通过", r.is_ok(), "");
    }

    // ---- 判据：双侧定位 ----

    // 参数个数不符：定义处 + 调用处同时给
    {
        let t = test_table();
        let e = expand_input(&t, "ADD(1,2,3)", &cfg).unwrap_err();
        set.add(
            "C12-定位-参数不符双侧",
            e.code == "E_MACRO_ARITY"
                && e.def_pos == Some(5)
                && e.def_line == Some(2)
                && e.is_complete(),
            "",
        );
    }
    // 调用缺收括号显性
    {
        let t = test_table();
        let e = expand_input(&t, "ADD(1,2", &cfg).unwrap_err();
        set.add(
            "C12-定位-调用缺收括号",
            e.code == "E_MACRO_UNCLOSED_CALL" && e.is_complete(),
            "",
        );
    }

    // ---- 重定义裁定 ----

    // Warn 策略：覆盖 + 警告文本
    {
        let mut t = MacroTable::new();
        let d1 = parse_definition("V 1", 1, 0).unwrap();
        let d2 = parse_definition("V 2", 2, 3).unwrap();
        let w1 = t.define(d1, &cfg).unwrap();
        let w2 = t.define(d2, &cfg).unwrap();
        let r = expand_input(&t, "V", &cfg).unwrap();
        set.add(
            "C12-重定义-Warn覆盖带警告",
            w1.is_none() && w2.is_some() && r.text == "2",
            "",
        );
    }
    // Error 策略：拒绝重定义
    {
        let strict = MacroConfig { max_depth: 64, redef: RedefPolicy::Error };
        let mut t = MacroTable::new();
        let d1 = parse_definition("V 1", 1, 0).unwrap();
        let d2 = parse_definition("V 2", 2, 3).unwrap();
        let _ = t.define(d1, &strict);
        let e = t.define(d2, &strict).unwrap_err();
        set.add(
            "C12-重定义-Error拒绝",
            e.code == "E_MACRO_REDEF" && e.def_pos.is_some(),
            "",
        );
    }

    // ---- 位置保留与查表 ----

    // 展开产物记号携带双侧坐标（调用处 + 定义处）
    {
        let t = test_table();
        let r = expand_input(&t, "x = PI;", &cfg).unwrap();
        let from_macro = r.tokens.iter().find(|tk| tk.text == "314").unwrap();
        set.add(
            "C12-定位-产物双侧坐标",
            from_macro.call_pos == 4
                && from_macro.def_line == Some(1)
                && from_macro.def_pos == Some(0),
            "",
        );
    }
    // 多宏登记后 O(1) 查表仍准确（哈希桶）
    {
        let mut t = MacroTable::new();
        for k in 0..40u32 {
            let d = parse_definition(&alloc::format!("M{} {}", k, k), 1, 0).unwrap();
            let _ = t.define(d, &cfg);
        }
        let ok = (0..40u32).all(|k| {
            t.lookup(&alloc::format!("M{}", k))
                .map(|m| m.body == alloc::format!("{}", k))
                .unwrap_or(false)
        });
        set.add("C12-查表-哈希桶全中", ok, "");
    }
    // 未定义标识符原样保留
    {
        let t = test_table();
        let r = expand_input(&t, "notmacro", &cfg).unwrap();
        set.add("C12-边界-未定义保留", r.text == "notmacro", "");
    }
    // 错误三要素完整（全错误码变体）
    {
        let t = test_table();
        let errs = [
            expand_input(&t, "ADD(1,2,3)", &cfg).unwrap_err(),
            expand_input(&t, "ADD(1,2", &cfg).unwrap_err(),
            {
                // 深度变体：三条互异宏链超过上限 2
                let mut tc = MacroTable::new();
                for k in 0..5u32 {
                    let body = if k == 4 { alloc::format!("M{} 0", k) } else { alloc::format!("M{} M{}", k, k + 1) };
                    let d = parse_definition(&alloc::format!("M{} {}", k, body), k as usize, k as usize).unwrap();
                    let _ = tc.define(d, &cfg);
                }
                expand_input(&tc, "M0", &MacroConfig { max_depth: 2, redef: RedefPolicy::Warn }).unwrap_err()
            },
            parse_definition(" 314", 1, 0).unwrap_err(),
        ];
        set.add(
            "C12-边界-三要素完整",
            errs.iter().all(|e| e.is_complete()),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0412 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn vec12_checks_all_green() {
        let set = run_vec12_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec12 自检存在红项：{}/{} 绿，红项：{:?}",
            passed,
            passed + failed,
            set.red_items()
                .0
                .iter()
                .flatten()
                .filter(|c| !c.passed)
                .map(|c| c.name)
                .collect::<Vec<_>>()
        );
    }
}
