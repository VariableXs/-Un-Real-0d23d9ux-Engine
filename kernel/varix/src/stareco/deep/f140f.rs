//! 深化层三 · F140 本地化开放（2026-09-26 深化批次三）。
//!
//! 补深翻译工程面（主册 G-D-15）：词条 diff 引擎（两包增删改清单）、
//! 复数占位符核（目标语复数形数与 {n} 配套校验）、伪本地化生成器
//! （布局测试的确定性变体）、术语表守卫（禁模糊词扫描）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 词条 diff：两包 key-value 序列 → 增/删/改（值不同）
// ---------------------------------------------------------------------------

/// 输入为 (key, 值指纹) 对——值用指纹对比避免载体语言依赖。
pub struct L10nDiff {
    pub added: alloc::vec::Vec<&'static str>,
    pub removed: alloc::vec::Vec<&'static str>,
    pub changed: alloc::vec::Vec<&'static str>,
}

pub fn diff_packs(
    base: &[(&'static str, u64)],
    next: &[(&'static str, u64)],
) -> L10nDiff {
    let mut d = L10nDiff {
        added: alloc::vec::Vec::new(),
        removed: alloc::vec::Vec::new(),
        changed: alloc::vec::Vec::new(),
    };
    for (k, v) in base {
        match next.iter().find(|(n, _)| n == k) {
            None => d.removed.push(k),
            Some((_, v2)) => {
                if v != v2 {
                    d.changed.push(k);
                }
            }
        }
    }
    for (k, _) in next {
        if !base.iter().any(|(b, _)| b == k) {
            d.added.push(k);
        }
    }
    d
}

// ---------------------------------------------------------------------------
// 复数占位符核：{n} 词条必须配足目标语复数形数
// ---------------------------------------------------------------------------

/// 语种复数形数：zh=1（无形态变化）、en=2、ru=3（简化登记面）。
pub fn plural_forms_for(lang: &str) -> u8 {
    match lang {
        "zh" => 1,
        "en" => 2,
        "ru" => 3,
        _ => 2, // 未知语种按保守 2 形要求
    }
}

/// 校验：值含 {n} 时，形数必须 ≥ 语种要求；不含 {n} 时单值即可。
/// forms 为该词条提供的复数形数量。
pub fn plural_check(lang: &str, value_has_n: bool, forms: u8) -> Result<(), &'static str> {
    let need = plural_forms_for(lang);
    if value_has_n {
        if forms < need {
            return Err("复数形不足：{n} 词条形数低于语种要求");
        }
        if forms > need {
            return Err("复数形超额：超出语种要求的形数是死翻译");
        }
    } else if forms != 1 {
        return Err("非占位词条应单值");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 伪本地化：ASCII 词条 → 确定性重排变体（布局拉伸测试用）
// ---------------------------------------------------------------------------

/// 规则：小写字母重复一次 + 首尾包 [[ ]]。数字与大写原样。
/// 确定性：同输入同输出（无随机——对拍口径）。
pub fn pseudoloc(s: &str) -> alloc::vec::Vec<u8> {
    let mut out: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
    out.extend_from_slice(b"[[");
    for b in s.bytes() {
        out.push(b);
        if b.is_ascii_lowercase() {
            out.push(b);
        }
    }
    out.extend_from_slice(b"]]");
    out
}

/// 展开率核对：伪本地化长度 ≤ 原长×2+4（布局预留的机器面判据）。
pub fn expansion_budget_ok(s: &str) -> bool {
    let p = pseudoloc(s).len();
    p <= s.len() * 2 + 4
}

// ---------------------------------------------------------------------------
// 术语表守卫：禁模糊词扫描（值内出现即拒收）
// ---------------------------------------------------------------------------

pub const BANNED_FUZZY: [&str; 4] = ["大概", "可能吧", "若干", "etc"];

/// 扫描值是否含禁词（子串口径——翻译值审读机器面）。
pub fn banned_word_hit(value: &str) -> Option<&'static str> {
    for w in BANNED_FUZZY.iter() {
        if value.contains(w) {
            return Some(w);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F140F_TAG: &str = "stareco-F140-deep3";

pub fn run_f140_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new(F140F_TAG);

    // 词条 diff
    let base = [("ui.open", 100u64), ("ui.save", 200), ("ui.exit", 300)];
    let next = [("ui.open", 100), ("ui.save", 999), ("ui.new", 400)];
    let d = diff_packs(&base, &next);
    set.add("f140f added", d.added == alloc::vec!["ui.new"], "新增 key 检出");
    set.add("f140f removed", d.removed == alloc::vec!["ui.exit"], "删除 key 检出");
    set.add("f140f changed", d.changed == alloc::vec!["ui.save"], "值变更检出");
    set.add("f140f noop", diff_packs(&base, &base).added.is_empty() && diff_packs(&base, &base).changed.is_empty(), "同包零差异");

    // 复数核
    set.add("f140f plural zh", plural_check("zh", true, 1).is_ok(), "中文单形足够");
    set.add("f140f plural en short", plural_check("en", true, 1).is_err(), "英文缺复数形拒绝");
    set.add("f140f plural en ok", plural_check("en", true, 2).is_ok(), "英文双形通过");
    set.add("f140f plural ru", plural_check("ru", true, 3).is_ok(), "俄语三形通过");
    set.add("f140f plural over", plural_check("en", true, 3).is_err(), "形数超额拒绝");
    set.add("f140f plain single", plural_check("en", false, 2).is_err(), "非占位词条多值拒绝");

    // 伪本地化
    let p = pseudoloc("ab1Z");
    set.add(
        "f140f pseudo bytes",
        p == alloc::vec![b'[', b'[', b'a', b'a', b'b', b'b', b'1', b'Z', b']', b']'],
        "小写重复+包夹规则",
    );
    set.add("f140f pseudo budget", expansion_budget_ok("hello world"), "展开率在预算内");
    set.add("f140f pseudo deterministic", pseudoloc("abc") == pseudoloc("abc"), "确定性输出");

    // 禁词
    set.add("f140f banned hit", banned_word_hit("数量若干个") == Some("若干"), "禁词命中");
    set.add("f140f banned clean", banned_word_hit("数量为三点五万").is_none(), "干净值通过");
    set.add("f140f banned etc", banned_word_hit("apple, etc") == Some("etc"), "etc 检出");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn diff_symmetry_shape() {
        let a = [("k", 1u64)];
        let b: [(&'static str, u64); 0] = [];
        let d = diff_packs(&a, &b);
        assert_eq!(d.removed, alloc::vec!["k"]);
        let d2 = diff_packs(&b, &a);
        assert_eq!(d2.added, alloc::vec!["k"]);
    }

    #[test]
    fn pseudo_unicode_passthrough() {
        // 非 ASCII 字节原样通过（翻译审读对象是英文骨架）。
        let p = pseudoloc("好a");
        assert_eq!(p.len(), 2 + 3 + 2 + 2);
        assert!(expansion_budget_ok("好a"));
    }
}
