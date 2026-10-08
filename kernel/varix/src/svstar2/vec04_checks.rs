//! VE-F0404 · 域自检（判据逐条对应，见 `vec04_keywords.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 完美哈希 → `C04-哈希-*`
//! - 版本裁定 → `C04-裁定-*`
//! - 历史保留 → `C04-历史-*`
//! - 编译期断言 → `C04-断言-*`（本 crate 能编译 = 编译期断言已过；运行期
//!   镜像自检提供可观测面）
//! - 错误路径（保留字误用报错带版本 / 版本未知保守）→ `C04-错误-*`
//!
//! 纯静态表校验，无时钟无 IO，回归可复现。

use super::vec04_keywords::*;
use crate::checks::CheckSet;

/// VE-F0404 域自检。
pub fn run_vec04_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec04");

    // ---- 判据一：完美哈希 ----

    // 全部关键字 O(1) 查找命中且下标对位。
    {
        let ok = KEYWORDS
            .iter()
            .enumerate()
            .all(|(i, kw)| lookup(kw) == Some(i));
        set.add("C04-哈希-全关键字命中", ok, "");
    }

    // 非关键字零误报（碰撞防护：槽位命中后逐字比对）。
    {
        let probes = ["ifx", "iff", "else2", "struct_", "fnx", "vars", "loopz", "truex", ""];
        let ok = probes.iter().all(|p| lookup(p).is_none());
        set.add("C04-哈希-非关键字零误报", ok, "");
    }

    // 槽位表完整性（运行期镜像编译期断言：非空槽 == 关键字数）。
    {
        let ok = table_selfcheck().is_ok();
        set.add("C04-哈希-槽位表完整", ok, "");
    }

    // ---- 判据二：版本裁定 ----

    // 升级词按版本不同（alias：V1 标识符 / V2 关键字）。
    {
        let ok = classify("alias", Some(LangVersion::V1Legacy)) == WordClass::Identifier
            && classify("alias", Some(LangVersion::V2Current)) == WordClass::Keyword;
        set.add("C04-裁定-升级词分版", ok, "");
    }

    // 降级词按版本不同（attribute：V1 关键字 / V2 历史保留）。
    {
        let ok = classify("attribute", Some(LangVersion::V1Legacy)) == WordClass::Keyword
            && classify("attribute", Some(LangVersion::V2Current)) == WordClass::HistoryReserved;
        set.add("C04-裁定-降级词分版", ok, "");
    }

    // 版本未知 → 按最新版保守（与显式 V2Current 裁定一致）。
    {
        let ok = classify("alias", None) == classify("alias", Some(LangVersion::conservative()))
            && classify("attribute", None) == WordClass::HistoryReserved
            && LangVersion::conservative() == LangVersion::V2Current;
        set.add("C04-裁定-未知版本保守", ok, "");
    }

    // 差异表数据自洽：只登记有变化的词，且逐词逐版裁定与登记状态一致。
    {
        let status_matches = |word: &str, v: LangVersion, s: WordStatus| {
            let want = match s {
                WordStatus::Keyword => WordClass::Keyword,
                WordStatus::HistoryReserved => WordClass::HistoryReserved,
                WordStatus::Identifier => WordClass::Identifier,
            };
            classify(word, Some(v)) == want
        };
        let ok = !VERSION_DIFFS.is_empty()
            && VERSION_DIFFS.iter().all(|d| {
                !d.word.is_empty()
                    && d.v1 != d.v2
                    && status_matches(d.word, LangVersion::V1Legacy, d.v1)
                    && status_matches(d.word, LangVersion::V2Current, d.v2)
            });
        set.add("C04-裁定-差异表在册", ok, "");
    }

    // ---- 判据三：历史保留 ----

    // 历史保留字被识别（识别但非关键字——V1 关键字 V2 保留）。
    {
        let ok = VERSION_DIFFS
            .iter()
            .filter(|d| d.v1 == WordStatus::Keyword && d.v2 == WordStatus::HistoryReserved)
            .all(|d| classify(d.word, Some(LangVersion::V2Current)) == WordClass::HistoryReserved);
        set.add("C04-历史-识别且非关键字", ok, "");
    }

    // 未来保留域与保留前缀生效。
    {
        let ok = classify("class", None) == WordClass::FutureReserved
            && classify("gl_Position", None) == WordClass::ReservedPrefix
            && classify("vx_builtin", None) == WordClass::ReservedPrefix
            && classify("my_var", None) == WordClass::Identifier;
        set.add("C04-历史-保留域生效", ok, "");
    }

    // ---- 判据四：编译期断言 ----

    // 编译期断言在册（本 crate 编译通过即断言已过；此处核对其存在性与
    // 运行期镜像一致——种子非零、槽位互异、表满）。
    {
        let ok = PERFECT_SEED > 0 && slots_unique(PERFECT_SEED) && table_selfcheck().is_ok();
        set.add("C04-断言-编译期已过运行期镜像", ok, "");
    }

    // 查找稳定性：同词重复查找结果恒定（确定性纪律）。
    {
        let a = lookup("uniform");
        let b = lookup("uniform");
        let ok = a == b && a.is_some();
        set.add("C04-断言-查找确定性", ok, "");
    }

    // ---- 错误路径 ----

    // 关键字误用作标识符 → 报错带版本裁定。
    {
        let e = check_identifier("alias", Some(LangVersion::V2Current));
        let ok = matches!(e, Err(KeywordError::ReservedMisuse { ref class, .. }) if *class == WordClass::Keyword)
            && e.as_ref().err().map(|x| x.describe().contains("V2Current")).unwrap_or(false);
        set.add("C04-错误-关键字误用带版本", ok, "");
    }

    // 历史保留字误用 → 报错提示版本语义与迁移建议。
    {
        let e = check_identifier("attribute", Some(LangVersion::V2Current));
        let ok = e.is_err()
            && e.as_ref()
                .err()
                .map(|x| x.describe().contains("历史保留") && x.describe().contains("迁移"))
                .unwrap_or(false);
        set.add("C04-错误-历史保留误用提示", ok, "");
    }

    // 保留前缀误用 → 报错点名前缀域归属。
    {
        let e = check_identifier("gl_myVar", None);
        let ok = e.is_err()
            && e.as_ref()
                .err()
                .map(|x| x.describe().contains("gl_"))
                .unwrap_or(false);
        set.add("C04-错误-前缀误用点名", ok, "");
    }

    // 合法标识符放行（正路径）。
    {
        let ok = check_identifier("my_color", None).is_ok()
            && check_identifier("albedo", Some(LangVersion::V1Legacy)).is_ok();
        set.add("C04-错误-合法标识符放行", ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec04_checks_all_green() {
        let set = run_vec04_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0404 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 完美哈希在内核 Rust 侧 O(1) 可用（编译期表 + 运行期查找）。
    #[test]
    fn vec04_perfect_hash_usable_in_kernel() {
        assert!(PERFECT_SEED > 0, "编译期种子搜索必须成功（const 断言兜底）");
        assert_eq!(lookup("struct"), KEYWORDS.iter().position(|w| *w == "struct"));
        assert_eq!(lookup("not_a_keyword"), None);
    }
}
