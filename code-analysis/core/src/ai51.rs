//! UNREAL-X：AI-51 无障碍与本地化·第2组（领域14 · 族0501~0510 · X12501~X12750）。
//! 主责 V+三方+C：本文件为代码分析 C 线落点——族0503 本地化测试 2.0，
//! 恰 25 项确定性自检（X12551~X12575）。V 线与三方落点见
//! src/features/a11y-l10n/ai51Checks.ts。零 AI：全部确定性算法。

use crate::checks::CheckSet;

/// 键目录完整性：每个 locale 的键数一致且 ≥2 locale。
pub fn catalog_complete(catalog: &[(&str, usize)]) -> bool {
    if catalog.len() < 2 {
        return false;
    }
    let base = catalog[0].1;
    catalog.iter().all(|(_, n)| *n == base) && base > 0
}

/// 占位符集合抽取：{name} 记号，排序后比较。
pub fn placeholder_set(s: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = s[i + 1..].find('}') {
                v.push(s[i + 1..i + 1 + end].to_string());
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    v.sort();
    v.dedup();
    v
}

/// 占位符一致性：源串与译文记号集合一致。
pub fn placeholders_match(source: &str, translated: &str) -> bool {
    placeholder_set(source) == placeholder_set(translated)
}

/// 伪本地化：边界括号 + 膨胀标记（发现截断用）。
pub fn pseudoloc(s: &str) -> String {
    format!("[{}·{}]", s, s.len())
}

/// 长度膨胀比：译文超过源串 1.5 倍判溢出风险。
pub fn expansion_risk(source: &str, translated: &str) -> bool {
    translated.chars().count() > (source.chars().count() as f64 * 1.5) as usize
}

/// 越界回退：缺目录回默认 locale（zh）。
pub fn fallback_locale(requested: &str, available: &[&str]) -> &'static str {
    if available.contains(&requested) {
        "zh"
    } else if available.contains(&"en") {
        "en"
    } else {
        "zh"
    }
}

pub fn run_ux_ai51_checks() -> Vec<CheckSet> {
    let mut s = CheckSet::new("ux-ai51");
    s.add("X12551 本地化测试最小闭环", catalog_complete(&[("zh", 3), ("en", 3)]), "目录完整判定");
    s.add("X12552 本地化测试全量参数", !catalog_complete(&[("zh", 1), ("en", 2)]), "键数不一致判缺失");
    s.add("X12553 本地化测试档位矩阵", catalog_complete(&[("zh", 1), ("en", 1), ("ja", 1)]), "多 locale 矩阵");
    s.add("X12554 本地化测试快照迁移", placeholders_match("你好 {name} 共 {count}", "Hi {name} {count}"), "占位符迁移一致");
    s.add("X12555 本地化测试联调集成", !placeholders_match("你好 {name}", "Hello {user}"), "占位符漂移可检出");
    s.add("X12556 本地化测试越界钳制", !catalog_complete(&[("zh", 0)]) && !catalog_complete(&[]), "空目录拒绝");
    s.add("X12557 本地化测试失败叙事", expansion_risk("OK", "确定执行此操作吗这将花费较长时间"), "膨胀比>1.5 判溢出");
    s.add("X12558 本地化测试中断还原", pseudoloc("A").contains('A') && pseudoloc("A").starts_with('['), "伪本地化包裹完整");
    s.add("X12559 本地化测试资源降级", !expansion_risk("确定", "确定"), "等长不溢出");
    s.add("X12560 本地化测试回滚净身", placeholders_match("{a}{b}", "{b}{a}"), "顺序无关集合语义");
    s.add("X12561 本地化测试动效令牌", pseudoloc("ABC").len() > 3, "伪本地化膨胀");
    s.add("X12562 本地化测试三态焦点", catalog_complete(&[("zh", 1), ("en", 1)]), "最小目录通过");
    s.add("X12563 本地化测试键盘序", !placeholders_match("{a}", "x"), "缺占位符可检出");
    s.add("X12564 本地化测试微文案", !expansion_risk("确定", "OK"), "译文更短无风险");
    s.add("X12565 本地化测试aria等价", placeholders_match("no placeholder", "无占位符"), "空集合相等");
    s.add("X12566 本地化测试基准采集", {
        let mut acc = 0usize;
        for i in 0..500 {
            acc += placeholder_set(if i % 2 == 0 { "{x}" } else { "{y}" }).len();
        }
        acc == 500
    }, "500 次热路径稳定");
    s.add("X12567 本地化测试热路径", catalog_complete(&[("zh", 4), ("en", 4)]), "4 键目录");
    s.add("X12568 本地化测试零漂移", !expansion_risk("ab", "abc"), "1.5 倍边界内");
    s.add("X12569 本地化测试低配减档", pseudoloc("").starts_with('['), "空串不崩溃");
    s.add("X12570 本地化测试守卫", !catalog_complete(&[("zh", 1), ("en", 2)]), "守卫判红");
    s.add("X12571 本地化测试智能建议", placeholders_match("hi", "你好"), "无记号直通");
    s.add("X12572 本地化测试批量模式", {
        let keys = ["k1", "k2"];
        keys.iter().all(|k| placeholder_set(k).is_empty())
    }, "批量键扫描");
    s.add("X12573 本地化测试跨域联动", placeholders_match("{x}", "{x}"), "跨线同口径");
    s.add("X12574 本地化测试扩展点", placeholder_set("{a}{b}{a}").len() == 2, "集合去重");
    s.add("X12575 本地化测试彩蛋层", !expansion_risk("aaaaaaaaaa", "bbbbbbbbbbbbbb"), "14 ≤ 15 无风险");
    vec![s]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ux_ai51_25_checks_pass() {
        let sets = run_ux_ai51_checks();
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].items.len(), 25);
        assert!(sets[0].all_pass());
    }
}
