//! VE-F0401 · 域自检（判据逐条对应，见 `vec01_constitution.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 原则六条全部可追溯到具体构造 → `C01-原则-*`
//! - 三方收编策略定义 → `C01-收编-*`
//! - 与 VE-C 接口冻结书一致 → `C01-冻结-*`
//! - 总纲评审通过 → `C01-评审-*`
//! - 判据引用链建立 → `C01-引用链-*`
//! - 族谱归一定位 → `C01-族谱-*`
//! - 错误路径注入（占位构造/断链引用/覆盖拒绝）→ `C01-注入-*`
//!
//! 纯静态注册表校验，无时钟无 IO，回归可复现。

use super::vec01_constitution::*;
use crate::checks::CheckSet;

/// VE-F0401 域自检。
pub fn run_vec01_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec01");
    let c = Constitution::standard();

    // ---- 判据一：原则六条全部可追溯到具体构造 ----

    // 原则数恰为六（总量铁律在总纲的投影：一条不多一条不少）。
    set.add(
        "C01-原则-六条在册",
        c.principles.len() == 6,
        "",
    );

    // 双向追踪：正向（每原则有构造）+ 反向（每构造指回原则且双向一致）。
    {
        let issues = c.check_traceability();
        set.add(
            "C01-原则-双向可溯无断链",
            issues.is_empty(),
            "",
        );
    }

    // 每条原则的落地构造语义里能找到该原则的关键词（构造真的在实现原则，
    // 不是挂名——抽查锚点点名的三条：绑定即校验/浮点显式/编译期报错）。
    {
        let spot = [
            (PrincipleId::MemorySafety, "绑定"),
            (PrincipleId::Determinism, "浮点"),
            (PrincipleId::PlatformHonesty, "编译期"),
        ];
        let ok = spot.iter().all(|(p, kw)| {
            c.catalog
                .by_principle(*p)
                .iter()
                .any(|con| con.semantics.contains(kw))
        });
        set.add("C01-原则-构造语义落点", ok, "");
    }

    // 构造目录覆盖五类构造（类型/语句/内建/注记/模块——落地不能只在一个面）。
    {
        let mut seen = [false; 5];
        for i in 0..c.catalog.len() {
            match c.catalog.entries[i].kind {
                ConstructKind::Type => seen[0] = true,
                ConstructKind::Statement => seen[1] = true,
                ConstructKind::Builtin => seen[2] = true,
                ConstructKind::Annotation => seen[3] = true,
                ConstructKind::Module => seen[4] = true,
            }
        }
        set.add(
            "C01-原则-构造五类覆盖",
            seen.iter().all(|s| *s),
            "",
        );
    }

    // ---- 判据二：三方收编策略定义 ----

    // 三族策略齐备（GLSL/HLSL/WGSL 各一份，少一族即红）。
    {
        let ok = SourceFamily::ALL
            .iter()
            .all(|f| c.ingestion.iter().any(|s| s.family == *f))
            && c.ingestion.len() == 3;
        set.add("C01-收编-三族齐备", ok, "");
    }

    // 每族策略五要素齐 + 归一路径合法 + 已知缺口非空（诚实清单）。
    {
        let ok = c.check_ingestion().is_empty()
            && c.ingestion
                .iter()
                .all(|s| !s.known_gaps.is_empty() && !s.forbidden.is_empty());
        set.add("C01-收编-五要素齐", ok, "");
    }

    // 禁止事项含红线两条（不发明私有格式 / 不魔改兼容——VE 册铁律）。
    {
        let ok = c.ingestion.iter().all(|s| {
            REDLINE_FORBIDDEN
                .iter()
                .all(|r| s.forbidden.iter().any(|f| f.contains(r)))
        });
        set.add("C01-收编-红线在册", ok, "");
    }

    // HLSL 存量收编入口语义显式（锚点：入口像 HLSL 一样能收编存量）。
    {
        let ok = c
            .ingestion
            .iter()
            .find(|s| s.family == SourceFamily::Hlsl)
            .map(|s| s.entry_semantics.contains("入口") && s.entry_semantics.contains("收编"))
            .unwrap_or(false);
        set.add("C01-收编-HLSL存量入口", ok, "");
    }

    // ---- 判据三：与 VE-C 接口冻结书一致 ----

    // 八族后端逐项一致（族数一致 + 族名逐项一致 + 无多无漏）。
    {
        let fz = c.check_freeze_consistency();
        set.add(
            "C01-冻结-八族后端一致",
            fz.backends_match && fz.conflicts.is_empty(),
            "",
        );
    }

    // 软渲测试靶契约登记且含像素对拍语义（对拍红线的落地方式）。
    {
        let fz = c.check_freeze_consistency();
        set.add(
            "C01-冻结-软渲测试靶",
            fz.softrender_contract_ok,
            "",
        );
    }

    // 前端接口冻结书版本登记（非空且带版本号形态）。
    {
        let fz = c.check_freeze_consistency();
        set.add(
            "C01-冻结-前端接口版本",
            fz.frontend_interface_ok
                && c.freeze.frontend_interface_version.contains("v1"),
            "",
        );
    }

    // ---- 判据四：总纲评审通过 ----

    // 标准评审：全项通过 + 双签齐 → 裁决 Passed。
    {
        let (v, issues) = c.check_review();
        set.add(
            "C01-评审-双签通过",
            v == ReviewVerdict::Passed && issues.is_empty(),
            "",
        );
    }

    // 评审项覆盖五条判据（至少五项，走过场的评审不算评审）。
    set.add(
        "C01-评审-项覆盖判据",
        c.review.item_count() >= 5,
        "",
    );

    // ---- 判据五：判据引用链建立 ----

    // 条款编号制在册（≥10 条，每条摘要自足非空）。
    {
        let ok = c.registry.clause_count() >= 10
            && c.registry
                .clauses()
                .iter()
                .all(|cl| !cl.digest.is_empty() && cl.id.starts_with("VS-G-"));
        set.add("C01-引用链-条款编号制", ok, "");
    }

    // C 域十组每组引用 ≥1 条款（链覆盖全组）。
    {
        let ok = c
            .registry
            .group_coverage()
            .iter()
            .all(|(_, n)| *n >= 1)
            && c.registry.ref_count() == C_DOMAIN_GROUPS.len();
        set.add("C01-引用链-十组全覆盖", ok, "");
    }

    // 链双向：条款可反查引用组（抽查六条原则条款都有引用方）。
    {
        let ok = ["VS-G-01", "VS-G-02", "VS-G-03", "VS-G-04", "VS-G-05", "VS-G-06"]
            .iter()
            .all(|id| !c.registry.citing_groups(id).is_empty());
        set.add("C01-引用链-双向反查", ok, "");
    }

    // ---- 族谱归一定位 ----

    // 三方源先归一（所有收编路径 DedicatedFrontend + 定位声明四句在册）。
    {
        let ok = c.ingestion.iter().all(|s| s.normalization == NormalizationPath::DedicatedFrontend)
            && c.family_position.len() == 4
            && c.family_position[3].contains("先归一");
        set.add("C01-族谱-先归一定位", ok, "");
    }

    // ---- 错误路径注入（零静默 + 拒绝带建议） ----

    // 注入：占位构造（空语义）注册被拒且给修正建议。
    {
        let r = Construct::new("VS-C-99", "占位", PrincipleId::MemorySafety, ConstructKind::Type, "");
        let ok = matches!(r, Err(ConstitError::EmptyConstructSpec { .. }));
        set.add("C01-注入-占位构造拒绝", ok, "");
    }

    // 注入：引用不存在的条款（断链）被拒且给修正建议。
    {
        let mut reg = ClauseRegistry::standard();
        let r = reg.add_ref(GroupRef {
            group: "词法",
            clause_ids: alloc::vec!["VS-G-99"],
        });
        let ok = matches!(r, Err(ConstitError::DanglingClauseRef { ref clause, .. }) if clause == "VS-G-99");
        set.add("C01-注入-断链引用拒绝", ok, "");
    }

    // 注入：条款号原地覆盖被拒（条款只增不改，修订走升版）。
    {
        let mut reg = ClauseRegistry::standard();
        let r = reg.register(Clause {
            id: "VS-G-01",
            title: "覆盖尝试",
            digest: "试图覆盖既有条款——必须被拒绝",
            origin: ClauseOrigin::Governance,
        });
        let ok = matches!(r, Err(ConstitError::DuplicateClauseId { .. }));
        set.add("C01-注入-条款覆盖拒绝", ok, "");
    }

    // 注入：收编策略缺红线被拒。
    {
        let r = IngestionStrategy::new(
            SourceFamily::Wgsl,
            "入口语义非空",
            NormalizationPath::DedicatedFrontend,
            "版本覆盖非空",
            alloc::vec!["缺口A"],
            alloc::vec!["无红线条款"],
        );
        let ok = matches!(r, Err(ConstitError::RedlineMissing { .. }));
        set.add("C01-注入-缺红线拒绝", ok, "");
    }

    // 注入：评审缺双签 → 裁决 MissingSignatures（否决路径覆盖）。
    {
        let items = alloc::vec![ReviewItem {
            id: "RV-X",
            question: "注入用单签评审",
            passed: true,
            remedy: "",
        }];
        let single = ReviewRecord::new(items, alloc::vec!["施工AI"]).expect("注入评审合法");
        set.add(
            "C01-注入-缺签不过",
            single.verdict() == ReviewVerdict::MissingSignatures,
            "",
        );
    }

    // 注入：评审有红项 → 裁决 Rejected 且整改建议在列。
    {
        let items = alloc::vec![
            ReviewItem { id: "RV-1", question: "绿项", passed: true, remedy: "" },
            ReviewItem { id: "RV-2", question: "红项", passed: false, remedy: "补整改" },
        ];
        let bad = ReviewRecord::new(items, alloc::vec!["施工AI", "验收AI"]).expect("注入评审合法");
        let (v, issues) = {
            let mut cons = Constitution::standard();
            cons.review = bad;
            cons.check_review()
        };
        let ok = v == ReviewVerdict::Rejected
            && issues.iter().any(|i| i.suggestion.contains("补整改"));
        set.add("C01-注入-红项否决", ok, "");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vec01_checks_all_green() {
        let set = run_vec01_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0401 自检存在红项：{}/{} 绿",
            passed,
            passed + failed
        );
    }

    /// 落位纪律自检：总纲聚合体在内核 Rust 侧可构建可校验。
    #[test]
    fn vec01_constitution_builds_and_verifies() {
        let c = Constitution::standard();
        assert_eq!(c.principles.len(), 6, "原则六条");
        assert_eq!(c.catalog.len(), 18, "构造目录预置 18 条");
        assert_eq!(c.ingestion.len(), 3, "三方收编策略");
        assert!(c.registry.clause_count() >= 10, "条款编号制在册");
        assert!(
            c.verification_issues().is_empty(),
            "标准总纲全量校验零红项"
        );
    }
}
