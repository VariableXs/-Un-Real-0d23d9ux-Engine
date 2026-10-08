//! VE-F0410 · 域自检（判据逐条对应，见 `vec10_brace.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 栈检查 → `C10-配对-*`（三类括号全配对、嵌套深度记账）
//! - 双侧定位 → `C10-定位-*`（失配给开符+当前位置两侧）
//! - 作用域直供 → `C10-作用域-*`（花括号开合标记流带深度、供符号表期）
//! - 深度上限 → `C10-深度-*`（超限报错、上限可配）

use super::vec10_brace::*;
use crate::checks::CheckSet;

/// VE-F0410 域自检。
pub fn run_vec10_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec10");
    let cfg = BraceConfig::default_config();

    // ---- 判据：栈检查 ----

    // 三类括号全配对
    {
        let r = scan_braces("f(x[0]{y})", &cfg).unwrap();
        set.add(
            "C10-配对-三类全配对",
            r.pairs.len() == 3
                && r.pairs.iter().all(|p| p.open_pos < p.close_pos),
            "",
        );
    }
    // 嵌套深度记账：外层 depth=0 收，内层收时 depth=1
    {
        let r = scan_braces("{a[b(c)]}", &cfg).unwrap();
        let brace = r.pairs.iter().find(|p| p.kind == BraceKind::Brace).unwrap();
        let paren = r.pairs.iter().find(|p| p.kind == BraceKind::Paren).unwrap();
        set.add(
            "C10-配对-深度记账",
            brace.depth == 0 && paren.depth == 2,
            "",
        );
    }
    // 三类各自独立配对不许混（先开先合）
    {
        let r = scan_braces("([{}])", &cfg).unwrap();
        set.add(
            "C10-配对-嵌套序正确",
            r.pairs.len() == 3
                && r.pairs[0].kind == BraceKind::Brace
                && r.pairs[2].kind == BraceKind::Paren,
            "",
        );
    }

    // ---- 判据：双侧定位 ----

    // 失配：双侧定位（开符与当前位置都给）
    {
        let e = scan_braces("{a)", &cfg).unwrap_err();
        set.add(
            "C10-定位-失配双侧",
            e.code == "E_BRACE_MISMATCH"
                && e.pos == 2
                && e.open_pos == Some(0)
                && e.is_complete(),
            "",
        );
    }
    // 孤悬收符：open_pos = None
    {
        let e = scan_braces(")x", &cfg).unwrap_err();
        set.add(
            "C10-定位-孤悬收符无开侧",
            e.code == "E_BRACE_MISMATCH" && e.open_pos.is_none() && e.pos == 0,
            "",
        );
    }
    // 未闭合到 EOF：双侧（开符 + 输入末尾）
    {
        let e = scan_braces("f(g{x", &cfg).unwrap_err();
        set.add(
            "C10-定位-未闭合到EOF",
            e.code == "E_BRACE_UNCLOSED"
                && e.open_pos == Some(3)
                && e.pos == 5,
            "",
        );
    }
    // 行列换算正确（跨行后开符行列）
    {
        let src = "a\nb {";
        let e = scan_braces(src, &cfg).unwrap_err();
        let (l, c) = BraceError::line_col(src, e.open_pos.unwrap_or(0));
        set.add("C10-定位-行列换算", l == 2 && c == 3, "");
    }

    // ---- 交叉嵌套（错序分流） ----

    // 先花后圆错序：{ ( } → CROSSED，指向栈顶开符
    {
        let e = scan_braces("{ ( }", &cfg).unwrap_err();
        set.add(
            "C10-交叉-错序报错",
            e.code == "E_BRACE_CROSSED"
                && e.pos == 4
                && e.open_pos == Some(2)
                && e.next.contains("闭合"),
            "",
        );
    }
    // 普通失配与交叉的分流对照：栈内无同类开符 = MISMATCH
    {
        let e = scan_braces("[ }", &cfg).unwrap_err();
        set.add(
            "C10-交叉-与失配分流",
            e.code == "E_BRACE_MISMATCH",
            "",
        );
    }

    // ---- 判据：作用域直供 ----

    // 花括号开合标记流带深度（圆方括号不进作用域流）
    {
        let r = scan_braces("{ { } }", &cfg).unwrap();
        let d = |i: usize| r.scopes[i].depth;
        set.add(
            "C10-作用域-标记流深度",
            r.scopes.len() == 4
                && r.scopes[0].event == ScopeEvent::Open
                && r.scopes[1].event == ScopeEvent::Open
                && r.scopes[2].event == ScopeEvent::Close
                && r.scopes[3].event == ScopeEvent::Close
                && d(0) == 1 && d(1) == 2 && d(2) == 1 && d(3) == 0,
            "",
        );
    }
    // 标记流与配对记录一致（同一扫描两流，符号表期不重扫）
    {
        let r = scan_braces("x[0]{f(y)}", &cfg).unwrap();
        let opens = r.scopes.iter().filter(|m| m.event == ScopeEvent::Open).count();
        set.add(
            "C10-作用域-两流一致",
            opens == r.pairs.iter().filter(|p| p.kind == BraceKind::Brace).count()
                && r.pairs.len() == 3,
            "",
        );
    }
    // 标记位置即花括号字节位置（O(1) 直取）
    {
        let r = scan_braces("a{b}c", &cfg).unwrap();
        set.add(
            "C10-作用域-位置直取",
            r.scopes[0].pos == 1 && r.scopes[1].pos == 3,
            "",
        );
    }

    // ---- 判据：深度上限 ----

    // 默认上限内的合法深嵌套
    {
        let src = format!("{}", "{}".repeat(16));
        let r = scan_braces(&src, &cfg).unwrap();
        set.add("C10-深度-常规嵌套通过", r.scopes.len() == 32, "");
    }
    // 超限报错（上限可配：用 max=4 触发）
    {
        let small = BraceConfig { max_depth: 4 };
        let src = "(((((";
        let e = scan_braces(src, &small).unwrap_err();
        set.add(
            "C10-深度-超限报错",
            e.code == "E_BRACE_DEPTH" && e.pos == 4,
            "",
        );
    }
    // 恰好在上限内不报错（边界纪律：>= 才拒）
    {
        let small = BraceConfig { max_depth: 4 };
        let r = scan_braces("(((())))", &small);
        set.add("C10-深度-恰好上限通过", r.is_ok() && r.unwrap().pairs.len() == 4, "");
    }

    // ---- 保护与边界 ----

    // 字符串内括号不参与配对
    {
        let r = scan_braces("s = \")({[\"; x = 1;", &cfg).unwrap();
        set.add(
            "C10-保护-字符串括号忽略",
            r.pairs.is_empty(),
            "",
        );
    }
    // 原始串与注释内括号忽略
    {
        let r = scan_braces("r#\"(}\"# // (\n/* ) */ {}", &cfg).unwrap();
        set.add(
            "C10-保护-原始串与注释忽略",
            r.pairs.len() == 1 && r.pairs[0].kind == BraceKind::Brace,
            "",
        );
    }
    // 空输入
    {
        let r = scan_braces("", &cfg).unwrap();
        set.add("C10-边界-空输入", r.pairs.is_empty() && r.scopes.is_empty(), "");
    }
    // 错误三要素完整（全部错误变体）
    {
        let errs = [
            scan_braces("{a)", &cfg).unwrap_err(),
            scan_braces(")x", &cfg).unwrap_err(),
            scan_braces("{ ( }", &cfg).unwrap_err(),
            scan_braces("(((", &cfg).unwrap_err(),
            scan_braces("((((", &BraceConfig { max_depth: 2 }).unwrap_err(),
        ];
        set.add(
            "C10-边界-三要素完整",
            errs.iter().all(|e| e.is_complete()),
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0410 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn vec10_checks_all_green() {
        let set = run_vec10_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec10 自检存在红项：{}/{} 绿，红项：{:?}",
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
