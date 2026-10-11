//! VE-F0405 · 域自检（判据逐条对应，见 `vec05_ident.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 字符集规则 → `C05-字符集-*`
//! - 显性裁定（超长截断还是拒绝是声明出来的）→ `C05-裁定-*`
//! - 区分大小写（不折叠）→ `C05-大小写-*`
//! - 冲突提示（换名建议）→ `C05-冲突-*`

use super::vec05_ident::*;
use crate::checks::CheckSet;

use alloc::{format, vec};

/// VE-F0405 域自检。
pub fn run_vec05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec05");
    let rule = IdentifierRule::default_rule();
    let reg = KeywordRegistry::from_keywords(BUILTIN_KEYWORDS);

    // ---- 判据：字符集规则 ----

    // 合法：字母/下划线开头，字母数字下划线组成
    {
        let ok = normalize("my_var2", &rule).is_ok()
            && normalize("_hidden", &rule).is_ok()
            && normalize("V", &rule).is_ok();
        set.add("C05-字符集-合法名放行", ok, "");
    }
    // 首字符规则：数字开头拒绝且建议加前缀
    {
        let e = normalize("2fast", &rule);
        let ok = e.as_ref().err().map(|x| {
            x.code == "E_IDENT_FIRST"
                && x.pos == 0
                && x.next.contains("字母或下划线")
        }).unwrap_or(false);
        set.add("C05-字符集-数字开头拒绝带建议", ok, "");
    }
    // 中间非法字符：报错带位置与建议
    {
        let e = normalize("my-var", &rule);
        let ok = e.as_ref().err().map(|x| {
            x.code == "E_IDENT_CHAR" && x.pos == 2 && x.next.contains("下划线")
        }).unwrap_or(false);
        set.add("C05-字符集-非法字符带位置", ok, "");
    }
    // 非 ASCII（UTF-8 标识符）拒绝
    {
        let e = normalize("变量一", &rule);
        let ok = e.as_ref().err().map(|x| x.code == "E_IDENT_FIRST" || x.code == "E_IDENT_CHAR").unwrap_or(false);
        let e2 = normalize("café", &rule);
        // "café" 字节序：c,a,f,0xC3,0xA9 —— 首个非法字节在位置 3
        let ok2 = e2.as_ref().err().map(|x| x.code == "E_IDENT_CHAR" && x.pos == 3).unwrap_or(false);
        set.add("C05-字符集-非ASCII拒绝", ok && ok2, "");
    }
    // 空标识符拒绝
    {
        let e = normalize("", &rule);
        set.add(
            "C05-字符集-空名拒绝",
            e.as_ref().err().map(|x| x.code == "E_IDENT_EMPTY" && x.is_complete()).unwrap_or(false),
            "",
        );
    }

    // ---- 判据：显性裁定 ----

    // Reject 策略（默认）：超长拒绝并解释为什么
    {
        let long = "a".repeat(1025);
        let e = normalize(&long, &rule);
        let ok = e.as_ref().err().map(|x| {
            x.code == "E_IDENT_TOO_LONG"
                && x.why.contains("Reject")
                && x.next.contains("Truncate")
        }).unwrap_or(false);
        set.add("C05-裁定-默认超长拒绝", ok, "");
    }
    // Truncate 策略：截断但留告知（原名长/截后长）
    {
        let trunc_rule = IdentifierRule {
            max_len: 8,
            length_policy: LengthPolicy::Truncate,
        };
        let n = normalize("abcdefghij", &trunc_rule).unwrap();
        set.add(
            "C05-裁定-截断带告知",
            n.truncated
                && n.text == "abcdefgh"
                && n.original_len == 10
                && n.notices.len() == 1
                && n.notices[0].contains("8"),
            "",
        );
    }
    // 上限正好不触发裁定
    {
        let exact = "a".repeat(1024);
        set.add(
            "C05-裁定-上限正好放行",
            normalize(&exact, &rule).map(|n| !n.truncated).unwrap_or(false),
            "",
        );
    }

    // ---- 判据：区分大小写 ----

    {
        // normalize 原样保留大小写：Foo 与 foo 是两个不同标识符
        let a = normalize("Foo", &rule).unwrap();
        let b = normalize("foo", &rule).unwrap();
        let preserved = a.text == "Foo" && b.text == "foo" && a.text != b.text;
        // 同名不同大小写都合法（不会被当成重复折叠掉）
        set.add(
            "C05-大小写-不折叠语义声明",
            preserved && a.original_len == 3 && b.original_len == 3,
            "",
        );
    }

    // ---- 判据：冲突提示 ----

    // 关键字命中 + 换名建议合法、不撞关键字、不撞 F0404 保留前缀
    {
        let out = check_keyword_conflict("uniform", &rule, &reg).unwrap();
        let named = out.conflict
            && out.what.contains("uniform")
            && !out.suggestions.is_empty()
            && out.next.contains("换名");
        let suggestions_valid = out.suggestions.iter().all(|s| {
            normalize(s, &rule).is_ok()
                && !reg.is_keyword(s)
                && !s.starts_with("gl_")
                && !s.starts_with("vx_")
                && !s.starts_with("ve_")
        });
        set.add(
            "C05-冲突-关键字命中带换名建议",
            named && suggestions_valid,
            "",
        );
    }
    // F0404 联动：默认权威注册表 = GLSL 基线 + vec04 关键字 + 未来保留字
    {
        let auth = default_registry();
        let bigger = auth.count() > reg.count();
        // vec04 自有关键字与未来保留字都进权威表
        let linked = auth.is_keyword("fn")
            && auth.is_keyword("let")
            && auth.is_keyword("async")
            && auth.is_keyword("await")
            && auth.is_keyword("uniform");
        set.add(
            "C05-冲突-F0404关键字表合并",
            bigger && linked,
            "",
        );
    }
    // 无冲突零误报（普通标识符放行，无建议无报错）
    {
        let out = check_keyword_conflict("my_color", &rule, &reg).unwrap();
        set.add(
            "C05-冲突-普通名零误报",
            !out.conflict && out.suggestions.is_empty() && out.what.is_empty(),
            "",
        );
    }
    // 大小写敏感性延伸到关键字域：Uniform 不是关键字（区分大小写）
    {
        let out = check_keyword_conflict("Uniform", &rule, &reg).unwrap();
        set.add(
            "C05-冲突-关键字判定区分大小写",
            !out.conflict,
            "关键字是精确匹配——Uniform≠uniform，与语言区分大小写声明一致",
        );
    }
    // 非法标识符先报词法错（冲突检测不越过词法）
    {
        let e = check_keyword_conflict("if(x", &rule, &reg);
        set.add(
            "C05-冲突-非法名先报词法",
            e.as_ref().err().map(|x| x.code == "E_IDENT_CHAR").unwrap_or(false),
            "",
        );
    }
    // 查表 O(1) 的机检代理：最坏探测步数有界（GLSL 基线约 97 词 / 256 槽）
    {
        let auth = default_registry();
        set.add(
            "C05-冲突-查表探测步数有界",
            reg.worst_probe() <= 12
                && auth.worst_probe() <= 12
                && reg.count() >= 90
                && auth.count() > reg.count(),
            "线性探测表最坏步数超界即负载失衡（构建期缺陷）；权威表更大",
        );
    }

    // ---- 确定性 ----

    {
        let a = normalize("shader_main", &rule);
        let b = normalize("shader_main", &rule);
        let c = check_keyword_conflict("for", &rule, &reg).unwrap();
        let d = check_keyword_conflict("for", &rule, &reg).unwrap();
        set.add(
            "C05-确定-同输入同结果",
            a == b && c.suggestions == d.suggestions,
            "",
        );
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    /// 域自检必须全绿——红项即施工未完成。
    #[test]
    fn vec05_checks_all_green() {
        let set = run_vec05_checks();
        let (passed, failed) = set.tally();
        if !set.all_passed() {
            let (items, n) = set.red_items();
            let mut msg = format!("VE-C05 域自检红项：{}/{} 绿", passed, passed + failed);
            for it in items.iter().take(n) {
                if let Some(c) = it {
                    if !c.passed {
                        msg.push_str(&format!("\n  [红] {} — {}", c.name, c.detail));
                    }
                }
            }
            panic!("{}", msg);
        }
    }

    /// 区分大小写端到端：三个大小写变体在符号层面互不相同。
    #[test]
    fn case_is_semantic_not_noise() {
        let rule = IdentifierRule::default_rule();
        let reg = KeywordRegistry::from_keywords(BUILTIN_KEYWORDS);
        // "If" 不是关键字（关键字是 "if"），合法可用——大小写是语义
        assert!(reg.is_keyword("if"));
        assert!(!reg.is_keyword("If"));
        assert!(normalize("If", &rule).is_ok());
        let out = check_keyword_conflict("If", &rule, &reg).unwrap();
        assert!(!out.conflict);
    }

    /// 长度裁定是策略不是写死：同一输入两种策略两种结局。
    #[test]
    fn length_policy_is_explicit() {
        let long = "very_long_identifier_name_exceeding_limits";
        let reject = IdentifierRule {
            max_len: 10,
            length_policy: LengthPolicy::Reject,
        };
        let truncate = IdentifierRule {
            max_len: 10,
            length_policy: LengthPolicy::Truncate,
        };
        assert!(normalize(long, &reject).is_err());
        let n = normalize(long, &truncate).unwrap();
        assert_eq!(n.text, "very_long_".to_string().as_str());
        assert!(n.truncated && !n.notices.is_empty());
    }

    /// 关键字表自举：内置表非空、建议名永不再撞关键字。
    #[test]
    fn suggestions_never_collide_again() {
        let rule = IdentifierRule::default_rule();
        let reg = KeywordRegistry::from_keywords(BUILTIN_KEYWORDS);
        for key in BUILTIN_KEYWORDS.iter() {
            let out = check_keyword_conflict(key, &rule, &reg).unwrap();
            assert!(out.conflict, "{} 应判冲突", key);
            assert!(
                out.suggestions
                    .iter()
                    .all(|s| !reg.is_keyword(s) && normalize(s, &rule).is_ok()),
                "{} 的建议 {:?} 必须合法且不撞关键字",
                key,
                out.suggestions
            );
        }
    }
}
