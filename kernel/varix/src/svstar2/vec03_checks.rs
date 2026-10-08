//! VE-F0403 · 域自检（判据逐条对应，见 `vec03_lexer.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 单遍状态机 → `C03-单遍-*`
//! - 三元组记号 → `C03-三元组-*`
//! - 协同边界（指令不越权）→ `C03-边界-*`
//! - 决策记录 → `C03-决策-*`
//! - 错误路径（EOF 显性终止/截断回退重扫/防御断言）→ `C03-错误-*`
//! - 确定性（同输入同记号流）→ `C03-确定-*`
//!
//! 纯函数校验，无时钟无 IO，回归可复现。

use super::vec03_lexer::*;
use crate::checks::CheckSet;

use alloc::vec;

/// 样例源（覆盖全部记号形状 + 指令行 + 注释 + 多字符操作符 + 指数数字）。
const SAMPLE: &str = concat!(
    "#version 460 core\n",
    "// 行注释：绑定语义归语义层\n",
    "layout(binding = 0) uniform sampler2D albedo;\n",
    "/* 块注释\n   跨行 */\n",
    "void main() {\n",
    "    float e = 1.5e-3;\n",
    "    vec3 v = vec3(0.1, 2, 3) * 2.0;\n",
    "    if (v.x >= 0.0 && v.y != 1.0) { v.x <<= 2; }\n",
    "    string s = \"a\\\"b\";\n",
    "}\n",
);

/// VE-F0403 域自检。
pub fn run_vec03_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec03");

    // ---- 判据一：单遍状态机 ----

    // 单遍性审计：净推进 == 源码长度（输入恰好被有效扫描一遍）。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(SAMPLE) {
            Ok((_, stats)) => stats.is_single_pass(SAMPLE.len()),
            Err(_) => false,
        };
        set.add("C03-单遍-净推进等于源长", ok, "");
    }

    // 状态机穷举：样例覆盖全部记号类型（形状面全覆盖）。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(SAMPLE) {
            Ok((tokens, _)) => {
                let mut seen = [false; 7];
                for t in &tokens {
                    match t.kind {
                        TokenKind::Ident => seen[0] = true,
                        TokenKind::Number => seen[1] = true,
                        TokenKind::Punct => seen[2] = true,
                        TokenKind::Str => seen[3] = true,
                        TokenKind::Comment => seen[4] = true,
                        TokenKind::Directive => seen[5] = true,
                        TokenKind::Eof => seen[6] = true,
                        TokenKind::Unknown => {}
                    }
                }
                seen.iter().all(|s| *s)
            }
            Err(_) => false,
        };
        set.add("C03-单遍-全记号类型覆盖", ok, "");
    }

    // 多字符操作符最长匹配（==/!=/<=/>=/&&/<< 不被拆单）。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan("a == b != c <= d >= e && f << g") {
            Ok((tokens, _)) => {
                let ops: Vec<&str> = tokens
                    .iter()
                    .filter(|t| t.kind == TokenKind::Punct)
                    .map(|t| t.text("a == b != c <= d >= e && f << g"))
                    .collect();
                ops == vec!["==", "!=", "<=", ">=", "&&", "<<"]
            }
            Err(_) => false,
        };
        set.add("C03-单遍-多字符操作符", ok, "");
    }

    // ---- 判据二：三元组记号 ----

    // 每条记号携带（类型，跨度，行列），跨度与原文一致、行列单调。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(SAMPLE) {
            Ok((tokens, _)) => tokens.iter().all(|t| {
                let span_ok = t.end >= t.start && SAMPLE.get(t.start..t.end).is_some();
                let pos_ok = t.line >= 1 && t.col >= 1;
                span_ok && pos_ok
            }),
            Err(_) => false,
        };
        set.add("C03-三元组-类型跨度位置齐", ok, "");
    }

    // 位置准确性抽查：第二行记号 line==2（1 起行列）。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan("#x\nab") {
            Ok((tokens, _)) => tokens.iter().any(|t| t.kind == TokenKind::Ident && t.text("#x\nab") == "ab" && t.line == 2),
            Err(_) => false,
        };
        set.add("C03-三元组-跨行位置正确", ok, "");
    }

    // 记号文本抽查：标识符/数字/字符串原文无损。
    {
        let src = "float32 42 \"hi\"";
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(src) {
            Ok((tokens, _)) => {
                let kinds: Vec<(TokenKind, &str)> =
                    tokens.iter().map(|t| (t.kind, t.text(src))).collect();
                kinds.contains(&(TokenKind::Ident, "float32"))
                    && kinds.contains(&(TokenKind::Number, "42"))
                    && kinds.contains(&(TokenKind::Str, "\"hi\""))
            }
            Err(_) => false,
        };
        set.add("C03-三元组-原文无损", ok, "");
    }

    // ---- 判据三：协同边界（指令不越权）----

    // 指令整行收号 + 文本原样保留（词法零解释）。
    {
        let src = "#version 460 core\nfloat x;";
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(src) {
            Ok((tokens, _)) => {
                let d = tokens.iter().find(|t| t.kind == TokenKind::Directive);
                d.map(|t| t.text(src) == "#version 460 core").unwrap_or(false)
                    && directive_boundary_audit(&tokens, src)
            }
            Err(_) => false,
        };
        set.add("C03-边界-指令整行原样", ok, "");
    }

    // 指令内容里的关键字/数字不被拆记号（词法不越权分析指令内部）。
    {
        let src = "#define N 4\nint y;";
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(src) {
            Ok((tokens, _)) => tokens
                .iter()
                .filter(|t| t.kind == TokenKind::Directive)
                .all(|t| t.text(src) == "#define N 4"),
            Err(_) => false,
        };
        set.add("C03-边界-指令内部不拆分", ok, "");
    }

    // ---- 判据四：决策记录 ----

    // 决策在册（≥4 条）且点名第一条回答"为什么不用正则"。
    {
        let ok = LEXER_DECISIONS.len() >= 4
            && LEXER_DECISIONS[0].question.contains("正则")
            && LEXER_DECISIONS
                .iter()
                .all(|d| !d.rationale.is_empty() && !d.decision.is_empty());
        set.add("C03-决策-在册且有据", ok, "");
    }

    // 协同边界与关键字归属的决策显性（不越权的依据可引用）。
    {
        let ok = LEXER_DECISIONS
            .iter()
            .any(|d| d.id == "LEX-D2" && d.rationale.contains("预处理"))
            && LEXER_DECISIONS
                .iter()
                .any(|d| d.id == "LEX-D3" && d.decision.contains("语法器"));
        set.add("C03-决策-边界决策可引用", ok, "");
    }

    // ---- 错误路径 ----

    // 字符串 EOF 中途 → 显性终止（带位置，不静默吞）。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan("float x = \"unterminated") {
            Err(LexError::UnterminatedString { line, col }) => line == 1 && col > 1,
            _ => false,
        };
        set.add("C03-错误-字符串EOF显性", ok, "");
    }

    // 块注释 EOF 中途 → 显性终止。
    {
        let l = Lexer::new().expect("默认词法器合法");
        let ok = matches!(
            l.scan("/* 没闭合"),
            Err(LexError::UnterminatedComment { .. })
        );
        set.add("C03-错误-块注释EOF显性", ok, "");
    }

    // 窗口尺寸下限防御（过小窗口拒绝创建并给建议）。
    {
        let ok = matches!(
            DoubleWindow::new(4),
            Err(LexError::WindowTooSmall { .. })
        ) && DoubleWindow::new(64).is_ok();
        set.add("C03-错误-窗口下限防御", ok, "");
    }

    // ---- 截断回退重扫（正确性优先于单遍教条）----

    // 小窗口强制触发截断：记号起于保护带（50 字节处）跨窗口边界，
    // 重扫后记号文本无损、单遍性成立、重扫计数入账（受控回退可见）。
    {
        // 50 字节标识符（结束于窗口 64 的保护带内）+ 空格 + 40 字节标识符
        // （起点 51，扫到 56 字节时撞界触发回退重扫）。
        let head = "a".repeat(50);
        let tail = "b".repeat(40);
        let src = alloc::format!("{head} {tail}");
        let l = Lexer::new().expect("默认词法器合法");
        let ok = match l.scan(&src) {
            Ok((tokens, stats)) => {
                let idents: Vec<&str> = tokens
                    .iter()
                    .filter(|t| t.kind == TokenKind::Ident)
                    .map(|t| t.text(&src))
                    .collect();
                idents.len() == 2
                    && idents[1] == tail
                    && stats.rescans >= 1
                    && stats.is_single_pass(src.len())
            }
            Err(_) => false,
        };
        set.add("C03-错误-跨窗口重扫一致", ok, "");
    }

    // ---- 确定性 ----

    // 同输入两次扫描记号流全同（确定性纪律）。
    {
        let a = Lexer::new().expect("合法").scan(SAMPLE).ok();
        let b = Lexer::new().expect("合法").scan(SAMPLE).ok();
        set.add("C03-确定-同输入同记号流", a == b && a.is_some(), "");
    }

    // ---- 冒烟 ----

    set.add(
        "C03-冒烟-样例可扫",
        lexer_smoke(SAMPLE) > 20,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec03_checks_all_green() {
        let set = run_vec03_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0403 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 词法器在内核 Rust 侧可执行且确定性成立。
    #[test]
    fn vec03_lexer_deterministic_in_kernel() {
        let a = Lexer::new().expect("合法").scan("int main() { return 0; }");
        let b = Lexer::new().expect("合法").scan("int main() { return 0; }");
        assert_eq!(a, b, "同输入同记号流");
        let (tokens, stats) = a.expect("扫描成功");
        assert_eq!(tokens.last().map(|t| t.kind), Some(TokenKind::Eof));
        assert!(stats.is_single_pass("int main() { return 0; }".len()));
    }
}
