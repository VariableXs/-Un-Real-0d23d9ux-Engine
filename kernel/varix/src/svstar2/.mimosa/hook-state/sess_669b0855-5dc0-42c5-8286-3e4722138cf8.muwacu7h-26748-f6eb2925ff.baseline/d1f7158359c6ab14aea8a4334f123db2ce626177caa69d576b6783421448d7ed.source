//! VE-F0409 · 域自检（判据逐条对应，见 `vec09_operator.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 最长匹配 → `C09-匹配-*`（<<= vs << vs <；贪吃边界）
//! - 总表对齐 → `C09-总表-*`（优先级/结合性/节号锚/机检清零）
//! - 歧义登记 → `C09-歧义-*`（登记表查询/回填/防重）
//! - 语境裁定 → `C09-语境-*`（前缀/中缀歧义默认与改判口径）

use super::vec09_operator::*;
use crate::checks::CheckSet;

/// VE-F0409 域自检。
pub fn run_vec09_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec09");

    // ---- 判据：最长匹配 ----

    // <<= 命中优先于 << 与 <
    {
        let m = match_operator(b"a <<= b", 2).unwrap().unwrap();
        set.add(
            "C09-匹配-三字符最长优先",
            m.form == "<<=" && m.kind == OpKind::Assign && m.len == 3 && m.right_assoc,
            "",
        );
    }
    // << 命中优先于 <；< 单字符
    {
        let m = match_operator(b"x << y", 2).unwrap().unwrap();
        let m2 = match_operator(b"a < b", 2).unwrap().unwrap();
        set.add(
            "C09-匹配-长度层递降",
            m.form == "<<" && m.precedence == prec::SHIFT && m2.form == "<"
                && m2.precedence == prec::REL,
            "",
        );
    }
    // 贪吃边界：运算符后跟非运算符字符（"+,"只命中 +）
    {
        let m = match_operator(b"a+,b", 1).unwrap().unwrap();
        set.add(
            "C09-匹配-贪吃边界",
            m.form == "+" && m.len == 1 && m.precedence == prec::ADD,
            "",
        );
    }
    // 赋值族右结合全登记
    {
        let forms = ["=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>="];
        let all = forms.iter().all(|f| {
            lookup(f).map(|s| s.kind == OpKind::Assign && s.right_assoc).unwrap_or(false)
        });
        set.add("C09-匹配-赋值族十一形态", all, "");
    }

    // ---- 判据：总表对齐 ----

    // 优先级抽样：成员最高、赋值最低、逻辑与高于逻辑或、关系高于相等
    {
        let member = lookup(".").unwrap();
        let assign = lookup("=").unwrap();
        let and = lookup("&&").unwrap();
        let or = lookup("||").unwrap();
        let rel = lookup("<").unwrap();
        let eq = lookup("==").unwrap();
        set.add(
            "C09-总表-优先级序",
            member.precedence == 1
                && assign.precedence == 13
                && and.precedence < or.precedence
                && rel.precedence < eq.precedence,
            "",
        );
    }
    // 每条登记都有 F0402 节号锚
    {
        let all = OPERATORS.iter().all(|s| s.spec_ref.starts_with("F0402#S2-P"));
        set.add("C09-总表-节号锚齐全", all, "");
    }
    // 对齐机检清零（形态唯一/级别连续/右结合允许集/查表一致）
    {
        let v = table_alignment_check();
        set.add("C09-总表-机检清零", v.is_empty(), "");
    }
    // 六类全有登记
    {
        let has = |k: OpKind| OPERATORS.iter().any(|s| s.kind == k);
        set.add(
            "C09-总表-六类全集",
            has(OpKind::Arith)
                && has(OpKind::Bitwise)
                && has(OpKind::Logical)
                && has(OpKind::Relational)
                && has(OpKind::Member)
                && has(OpKind::Assign),
            "",
        );
    }

    // ---- 判据：歧义登记 ----

    // 模板 < 与小于号：登记在案，词法默认关系小于、语法期语境裁定
    {
        let e = ambiguity_lookup("<");
        set.add(
            "C09-歧义-模板小于号登记",
            e.len() == 1
                && e[0].phase == AdjudicationPhase::SyntaxContext
                && e[0].lexical_default == "关系小于",
            "",
        );
    }
    // 嵌套泛型 >> 与右移登记
    {
        let e = ambiguity_lookup(">>");
        set.add(
            "C09-歧义-嵌套泛型闭符登记",
            e.len() == 1 && e[0].desc.contains("泛型"),
            "",
        );
    }
    // 一元负号语境：- 登记为 Prefix 语境改判
    {
        let e = ambiguity_lookup("-");
        set.add(
            "C09-语境-一元负号改判",
            e.len() == 1
                && e[0].context == OpContext::Prefix
                && e[0].phase == AdjudicationPhase::SyntaxContext,
            "",
        );
    }
    // 未登记符号查询为空（登记表只认在案歧义）
    {
        set.add("C09-歧义-未登记符号空查询", ambiguity_lookup("@").is_empty(), "");
    }
    // 回填登记：合法回填成功、重复回填拒绝
    {
        let mut led = AmbiguityLedger::new();
        let entry = AmbiguityEntry {
            symbol: "?",
            desc: "条件运算符 vs 可空类型标记（假设语法期发现）",
            lexical_default: "无条件运算符登记",
            phase: AdjudicationPhase::SyntaxContext,
            context: OpContext::Infix,
        };
        let ok1 = led.backfill(entry).is_ok();
        let dup = led.backfill(entry);
        set.add(
            "C09-歧义-回填与防重",
            ok1 && led.backfilled().len() == 1 && dup.is_err(),
            "",
        );
    }

    // ---- 错误路径：未知符号带最近建议 ----

    {
        let e = match_operator(b"a @ b", 2).unwrap_err();
        set.add(
            "C09-错误-未知符号三要素",
            e.code == "E_OP_UNKNOWN" && e.pos == 2 && e.is_complete(),
            "",
        );
    }
    // 同首字符候选：& 系（& && &=）都进建议
    {
        let near = suggest_near(b'&');
        set.add(
            "C09-错误-最近建议同首字符",
            near.contains(&"&") && near.contains(&"&&") && near.contains(&"&=")
                && near.len() == 3,
            "",
        );
    }
    // 非法运算符位置的非 ASCII 防护（多字节 UTF-8 字符不是运算符）
    {
        let e = match_operator("a é b".as_bytes(), 2).unwrap_err();
        set.add("C09-错误-非ASCII拒绝", e.code == "E_OP_UNKNOWN", "");
    }

    // ---- 边界 ----

    // 空输入与末尾越界
    {
        let none_empty = match_operator(b"", 0).unwrap();
        let none_oob = match_operator(b"ab", 5).unwrap();
        set.add("C09-边界-空与越界", none_empty.is_none() && none_oob.is_none(), "");
    }
    // 合法运算符逐条可查（总表全表 roundtrip）
    {
        let all = OPERATORS
            .iter()
            .all(|s| lookup(s.form).map(|g| g.form == s.form).unwrap_or(false));
        set.add("C09-边界-全表roundtrip", all, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    /// VE-F0409 全绿闸：红项逐行枚举（定位用），零红才算过。
    #[test]
    fn vec09_checks_all_green() {
        let set = run_vec09_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "vec09 自检存在红项：{}/{} 绿，红项：{:?}",
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
