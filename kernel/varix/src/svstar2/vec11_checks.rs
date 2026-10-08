//! VE-F0411 · 域自检（判据逐条对应，见 `vec11_prepro.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 指令转预处理 → `C11-识别-*`（识别表 9 指令、未知指令带建议）
//! - 续行归并 → `C11-续行-*`（归并成单逻辑行、EOF 续行报错）
//! - 单一实现 → `C11-复用-*`（指令体记号来自主词法 vec03）
//! - 双流分离 → `C11-双流-*`（指令流×代码流、位置保真、行中井号裁定）

use super::vec03_lexer::TokenKind;
use super::vec11_prepro::*;
use crate::checks::CheckSet;

/// VE-F0411 域自检。
pub fn run_vec11_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec11");

    // ---- 判据：指令转预处理（识别表） ----

    // 识别表 9 指令全注册且查表一致
    {
        let all = DIRECTIVES.iter().all(|d| lookup_directive(d.name()) == Some(*d));
        set.add(
            "C11-识别-九指令注册",
            DIRECTIVES.len() == 9 && all,
            "",
        );
    }
    // include / define / 条件族类别正确
    {
        set.add(
            "C11-识别-类别分派",
            lookup_directive("include") == Some(DirectiveKind::Include)
                && lookup_directive("define") == Some(DirectiveKind::Define)
                && lookup_directive("ifdef") == Some(DirectiveKind::Ifdef)
                && lookup_directive("endif") == Some(DirectiveKind::Endif)
                && lookup_directive("definex").is_none(),
            "",
        );
    }
    // 指令行进指令流，kind/body 解析
    {
        let s = split_streams("#define MAX 100\n").unwrap();
        set.add(
            "C11-识别-define解析",
            s.directives.len() == 1
                && s.directives[0].kind == DirectiveKind::Define
                && s.directives[0].body == "MAX 100"
                && s.directives[0].name == "define",
            "",
        );
    }
    // 未知指令：报错带已注册指令集建议
    {
        let e = split_streams("#includeall x\n").unwrap_err();
        set.add(
            "C11-识别-未知指令带建议",
            e.code == "E_PP_UNKNOWN" && e.next.contains("#include") && e.is_complete(),
            "",
        );
    }

    // ---- 判据：续行归并 ----

    // 两行 define 归并成单逻辑行（continuations=1、起始行记账）
    {
        let s = split_streams("#define MAX \\\n    100\nx = 1;\n").unwrap();
        set.add(
            "C11-续行-归并单逻辑行",
            s.directives.len() == 1
                && s.directives[0].continuations == 1
                && s.directives[0].line == 1
                && s.directives[0].body == "MAX 100",
            "",
        );
    }
    // 逻辑行归并结果（不续行的普通行为原样）
    {
        let lines = merge_continuations("a\nb\n").unwrap();
        set.add(
            "C11-续行-无续行原样",
            lines.len() == 2 && lines[0].text == "a" && lines[0].continuations == 0,
            "",
        );
    }
    // 续行后 EOF 显性报错
    {
        let e = merge_continuations("a \\\n").unwrap_err();
        set.add(
            "C11-续行-EOF显性报错",
            e.code == "E_PP_CONT_EOF" && e.is_complete(),
            "",
        );
    }

    // ---- 判据：单一实现（复用主词法） ----

    // 指令体记号来自主词法：define 体产出 Ident/Number 记号
    {
        let s = split_streams("#define MAX 100\n").unwrap();
        let t = &s.directives[0].tokens;
        set.add(
            "C11-复用-主词法记号流",
            t.len() >= 2
                && t[0].kind == TokenKind::Ident
                && t[1].kind == TokenKind::Number,
            "",
        );
    }
    // include 体字符串记号复用同一词法
    {
        let s = split_streams("#include \"x.h\"\n").unwrap();
        let t = &s.directives[0].tokens;
        set.add(
            "C11-复用-字符串记号",
            t.iter().any(|tk| tk.kind == TokenKind::Str),
            "",
        );
    }

    // ---- 判据：双流分离 ----

    // 指令行与代码行各进各流、位置保真
    {
        let s = split_streams("#define A 1\nint x;\n").unwrap();
        set.add(
            "C11-双流-两流分流",
            s.directives.len() == 1
                && s.code.len() == 1
                && s.code[0].text == "int x;"
                && s.code[0].line == 2
                && s.directives[0].line == 1,
            "",
        );
    }
    // 行中井号：代码流内报 E_PP_POSITION
    {
        let e = split_streams("x = a # b\n").unwrap_err();
        set.add(
            "C11-双流-行中井号报错",
            e.code == "E_PP_POSITION" && e.is_complete(),
            "",
        );
    }
    // 语境裁定：define 体内的井号（字符串化语境）合法保留
    {
        let s = split_streams("#define STR(x) #x\n").unwrap();
        set.add(
            "C11-双流-define体内井号合法",
            s.directives.len() == 1 && s.directives[0].body.contains('#'),
            "",
        );
    }

    // ---- 边界 ----

    // 空输入双流皆空
    {
        let s = split_streams("").unwrap();
        set.add("C11-边界-空输入", s.directives.is_empty() && s.code.is_empty(), "");
    }
    // 纯代码输入全进代码流
    {
        let s = split_streams("a\nb\nc\n").unwrap();
        set.add("C11-边界-纯代码", s.code.len() == 3 && s.directives.is_empty(), "");
    }
    // 错误三要素完整（全错误码变体）
    {
        let errs = [
            split_streams("#bad x\n").unwrap_err(),
            split_streams("x # y\n").unwrap_err(),
            merge_continuations("a\\\n").unwrap_err(),
        ];
        set.add("C11-边界-三要素完整", errs.iter().all(|e| e.is_complete()), "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0411 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn vec11_checks_all_green() {
        let set = run_vec11_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec11 自检存在红项：{}/{} 绿，红项：{:?}",
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
