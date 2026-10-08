//! VE-F0605 · 域自检（判据逐条对应，见 `ved05_isolation.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 作用域定义 → `D05-作用域-语义文档在册`
//! - 条件全集（四条件各自独立触发） → `D05-条件-四条件独立触发`、`D05-条件-位图归因与F0603同源`
//! - 声明冲突→显式声明胜出 → `D05-冲突-显式声明胜出`
//! - 条件误触发→清单复审 → `D05-复审-误触发检出`
//! - 隔离组嵌套→递归语义明确（内层先闭合） → `D05-递归-内层先闭合`
//! - 语义实现分离 → `D05-分离-无执行面`
//! - 判定 O(1) 每层 / 嵌套合成 O(深度) → `D05-递归-内层先闭合`（深度账）
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use super::ved03_alpha::{CompositingPath, Opacity};
use super::ved05_isolation::*;
use crate::checks::CheckSet;

/// 无条件基础输入（不触发任何隐式条件）。
fn base() -> LayerIsolationInput {
    LayerIsolationInput {
        opacity: Opacity::new(1.0).unwrap(),
        blend_is_normal: true,
        effects_non_empty: false,
        mask_present: false,
        explicit: None,
    }
}

/// VE-F0605 域自检。
pub fn run_ved05_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-ved05");

    // ---- 作用域定义 ----

    {
        let ok = ISOLATION_SEMANTICS_DOC.contains("独立组")
            && ISOLATION_SEMANTICS_DOC.contains("整体再参与外部")
            && ISOLATION_SEMANTICS_DOC.contains("F0632")
            && NESTING_SEMANTICS_DOC.contains("内层先闭合");
        set.add("D05-作用域-语义文档在册", ok, "");
    }

    // ---- 条件全集：四条件各自独立触发 ----

    {
        // 每次只给一个条件，其余全关——各自独立触发才成立。
        let mut i = base();
        i.opacity = Opacity::new(0.5).unwrap();
        let c0 = decide_isolation(&i).source == IsolationSource::Implicit(ConditionMask(ConditionMask::OPACITY));
        let mut i = base();
        i.blend_is_normal = false;
        let c1 = decide_isolation(&i).source == IsolationSource::Implicit(ConditionMask(ConditionMask::BLEND));
        let mut i = base();
        i.effects_non_empty = true;
        let c2 = decide_isolation(&i).source == IsolationSource::Implicit(ConditionMask(ConditionMask::EFFECTS));
        let mut i = base();
        i.mask_present = true;
        let c3 = decide_isolation(&i).source == IsolationSource::Implicit(ConditionMask(ConditionMask::MASK));
        let none = decide_isolation(&base()).source == IsolationSource::None;
        set.add("D05-条件-四条件独立触发", c0 && c1 && c2 && c3 && none, "");
    }

    // ---- 位图归因 + 与 F0603 同源 ----

    {
        let mut i = base();
        i.opacity = Opacity::new(0.5).unwrap();
        i.effects_non_empty = true;
        i.mask_present = true;
        let d = decide_isolation(&i);
        let count3 = matches!(d.source, IsolationSource::Implicit(m) if m.count() == 3);
        let names_ok = matches!(d.source, IsolationSource::Implicit(m) if m.names().len() == 3);
        // C0 判定与上游 F0603 的 GroupComposite 同源（半透明触发/不透明不触发）。
        let semi = Opacity::new(0.5).unwrap();
        let f0603_semi = semi.path() == CompositingPath::GroupComposite;
        let c0_aligned = f0603_semi
            && decide_isolation(&LayerIsolationInput { opacity: semi, ..base() }).isolated;
        set.add("D05-条件-位图归因与F0603同源", count3 && names_ok && c0_aligned, "");
    }

    // ---- 声明冲突 → 显式声明胜出 ----

    {
        // 隐式全触发 + 显式豁免 → 不隔离，来源留痕。
        let mut i = base();
        i.opacity = Opacity::new(0.5).unwrap();
        i.blend_is_normal = false;
        i.effects_non_empty = true;
        i.mask_present = true;
        i.explicit = Some(false);
        let waive = decide_isolation(&i);
        let waived = !waive.isolated && waive.source == IsolationSource::ExplicitWaive;
        // 无隐式条件 + 显式 isolate → 隔离。
        let mut i = base();
        i.explicit = Some(true);
        let iso = decide_isolation(&i);
        let forced = iso.isolated && iso.source == IsolationSource::ExplicitIsolate;
        set.add("D05-冲突-显式声明胜出", waived && forced, "");
    }

    // ---- 条件误触发 → 清单复审 ----

    {
        let input = base();
        let claimed = IsolationDecision {
            isolated: true,
            source: IsolationSource::Implicit(ConditionMask(ConditionMask::BLEND)),
        };
        let caught = match audit_misfire("n=12", claimed, &input) {
            Some(fix) => !fix.after.isolated && fix.who == "n=12",
            None => false,
        };
        // 一致决策无修正；显式来源豁免复审。
        let consistent = audit_misfire("n=12", decide_isolation(&input), &input).is_none();
        let explicit_exempt = audit_misfire(
            "n=12",
            IsolationDecision { isolated: true, source: IsolationSource::ExplicitIsolate },
            &input,
        )
        .is_none();
        set.add("D05-复审-误触发检出", caught && consistent && explicit_exempt, "");
    }

    // ---- 递归明确：内层先闭合 + O(深度) 账 ----

    {
        let mut t = GroupNestingTracker::new();
        t.tick();
        t.open_group(1);
        t.open_group(2);
        t.open_group(3);
        let depth_ok = t.depth() == 3 && t.max_depth_seen() == 3;
        let out_of_order_rejected = t.close_group(1).is_err()
            && t.errors().iter().any(|(_, c, _)| *c == "E_INNER_MUST_CLOSE_FIRST");
        let lifo_ok = t.close_group(3).is_ok() && t.close_group(2).is_ok() && t.close_group(1).is_ok();
        let stray_rejected = t.close_group(9).is_err()
            && t.errors().iter().any(|(_, c, _)| *c == "E_CLOSE_WITHOUT_OPEN");
        set.add(
            "D05-递归-内层先闭合",
            depth_ok && out_of_order_rejected && lifo_ok && stray_rejected,
            "",
        );
    }

    // ---- 作用域规则：组成员区间解析 ----

    {
        let iso = IsolationDecision { isolated: true, source: IsolationSource::ExplicitIsolate };
        let plain = IsolationDecision { isolated: false, source: IsolationSource::None };
        let chain = alloc::vec![(1u64, iso), (2u64, plain), (3u64, plain), (4u64, iso), (5u64, plain)];
        let spans = resolve_group_spans(&chain);
        let two_groups = spans.len() == 2
            && spans[0] == GroupSpan { owner: 1, start_idx: 0, end_idx: 3 }
            && spans[1] == GroupSpan { owner: 4, start_idx: 3, end_idx: 5 };
        let plain_chain = alloc::vec![(1u64, plain), (2u64, plain)];
        let no_group = resolve_group_spans(&plain_chain).is_empty();
        set.add("D05-作用域-成员区间解析", two_groups && no_group, "");
    }

    // ---- 语义实现分离 ----

    {
        // 本条导出面无执行：决策是纯 Copy 数据，追踪器只记账不做合成；
        // 分离声明文本在册并点名 F0626。
        let d = decide_isolation(&base());
        let pure_copy = {
            let c = d; // Copy 语义 = 无副作用载体
            c == d
        };
        let doc_ok = SEMANTICS_ONLY_DOC.contains("F0626") && !SEMANTICS_ONLY_DOC.contains("执行归本条");
        set.add("D05-分离-无执行面", pure_copy && doc_ok, "");
    }

    set
}
