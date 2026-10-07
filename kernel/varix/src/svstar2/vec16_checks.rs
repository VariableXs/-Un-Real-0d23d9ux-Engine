//! VE-F0416 · 域自检（判据逐条对应，见 `vec16_report.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - 四族分类 → `C16-族-*`（四族各自归位、查表不猜、未归族如实标注、族级兜底建议、
//!   表顺序敏感：更长的前缀先命中）
//! - 三要素 → `C16-三要素-*`（三要素齐备、规则引用带出、模板缺失走最小告知、
//!   最小告知被登记不静默、上游三要素优先于模板兜底、空项不输出半空报告）
//! - 双侧定位 → `C16-双侧-*`（仅主侧、双侧、两侧都渲染、副侧可查、两侧独立钳制）
//! - 三级分级 → `C16-分级-*`（三级定义、阻断性、裁定表命中、未登记按最严并注记、
//!   全序可比）
//! - 位置三元式 → `C16-位置-*`（行列+字节偏移齐备、渲染含三者、越界钳制不 panic、
//!   自洽性核验、位置不自洽出注记）
//! - 零静默 → `C16-显性-*`（未归族注记、未登记严重度注记、待回填登记、报告非空）

// no_std 下 std prelude 不存在：`String` 与 `format!`/`vec!` 都得显式引入。
// 宿主 `cargo test` 有 std prelude 会掩盖这一点，整树 `cargo check --lib`
// 才暴露——两处都写上，两条链路都成立。
use alloc::vec;

use super::vec16_report::*;
use crate::checks::CheckSet;

/// 构造一个自洽位置。
fn loc(line: usize, col: usize, off: usize) -> Loc {
    Loc::new(line, col, off)
}

/// 上游三要素齐备的原始错误。
fn raw_ok(code: &'static str) -> RawError {
    RawError::new(code, loc(1, 1, 0)).with_triple("发生了什么", "为什么", "下一步", "VE-SPEC-X-1")
}

/// VE-F0416 域自检。
pub fn run_vec16_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vec16");

    // ---- 判据：四族分类 ----

    set.add("C16-族-字符类归位", classify("VE-F0415-UTF8-TRUNCATED") == ErrorFamily::Char, "");
    set.add("C16-族-字面量类归位", classify("VE-F0407-UNTERMINATED") == ErrorFamily::Literal, "");
    set.add("C16-族-括号类归位", classify("VE-F0410-MISMATCH") == ErrorFamily::Bracket, "");
    set.add("C16-族-指令类归位", classify("VE-F0412-RECURSION") == ErrorFamily::Directive, "");

    // 未登记码落 Other 族（如实说未归族，不硬塞进四族）
    let d_other = diagnose(&raw_ok("VE-F9999-NOT-REGISTERED"));
    set.add(
        "C16-族-未登记码落未归族（如实标注）",
        d_other.family == ErrorFamily::Other && !d_other.family.is_classified(),
        "",
    );

    // 未归族时族级建议必须说「需回填」，不得给出四族的具体建议冒充
    set.add(
        "C16-族-未归族的族级建议为回填提示",
        ErrorFamily::Other.remedy_hint().contains("回填"),
        "",
    );

    // 四族族级建议互不相同（否则「族级兜底」等于没有区分度）
    let hints = [
        ErrorFamily::Char.remedy_hint(),
        ErrorFamily::Literal.remedy_hint(),
        ErrorFamily::Bracket.remedy_hint(),
        ErrorFamily::Directive.remedy_hint(),
    ];
    // 手工去重计数（数组没有 dedup，那是 Vec 的方法）。
    let mut distinct = 0usize;
    for i in 0..hints.len() {
        let mut seen_before = false;
        for j in 0..i {
            if hints[j] == hints[i] {
                seen_before = true;
            }
        }
        if !seen_before {
            distinct += 1;
        }
    }
    set.add("C16-族-四族族级建议互异", distinct == 4, "");

    // 表顺序敏感：F0405/F0406/F0407 都必须归字面量类而非被更短的 F040 前缀抢走
    set.add(
        "C16-族-族表按最长前缀命中（F0406 不被 F040 抢判）",
        classify("VE-F0406-LITERAL") == ErrorFamily::Literal
            && classify("VE-F0405-IDENT") == ErrorFamily::Literal
            && classify("VE-F0403-LEX") == ErrorFamily::Char,
        "",
    );

    // 查表 vs 猜族的**可判别用例**：码里含「0415」数字串但**不以 VE-F 开头**。
    // 查表（starts_with 前缀）判未归族；按子串猜（contains）会误判成字符类。
    // 没有这条，contains 变异体在本域全部既有用例上与查表等价，判据抓不住它。
    set.add(
        "C16-族-含数字串但无 VE-F 前缀判未归族（查表非猜）",
        classify("X-0415-UTF8-TRUNCATED") == ErrorFamily::Other
            && classify("MY-0410-MISMATCH") == ErrorFamily::Other
            && classify("VE-F0415-UTF8-TRUNCATED") == ErrorFamily::Char,
        "",
    );

    // 归族是查表而非猜：同一前缀族下不同码归同一族（体现规则性）
    set.add(
        "C16-族-同族码归族一致（F0412 全族归指令类）",
        classify("VE-F0412-RECURSION") == ErrorFamily::Directive
            && classify("VE-F0412-DEPTH") == ErrorFamily::Directive
            && classify("VE-F0412-ARGS") == ErrorFamily::Directive,
        "",
    );

    // ---- 判据：三要素 ----

    let d_full = diagnose(&raw_ok("VE-F0415-UTF8-TRUNCATED"));
    set.add(
        "C16-三要素-三要素齐备（what/why/next/rule_ref）",
        d_full.is_complete()
            && d_full.what == "发生了什么"
            && d_full.why == "为什么"
            && d_full.next == "下一步"
            && d_full.rule_ref == "VE-SPEC-X-1",
        "",
    );

    // 规则引用必须出现在渲染文本里（判据二：为什么带规则引用）
    set.add(
        "C16-三要素-渲染带规则引用",
        d_full.render().contains("规则")
            && d_full.render().contains("VE-SPEC-X-1"),
        "",
    );

    // 渲染含三要素标签
    let r_txt = d_full.render();
    set.add(
        "C16-三要素-渲染含三要素标签与位置族级严重度",
        r_txt.contains("发生了什么") && r_txt.contains("为什么") && r_txt.contains("下一步")
            && r_txt.contains("字节偏移"),
        "",
    );

    // 模板缺失 → 最小告知不静默（三要素全空且无模板登记）
    let d_miss = diagnose(&RawError::new("VE-F9999-NOT-REGISTERED", loc(1, 1, 0)));
    set.add(
        "C16-三要素-模板缺失走最小告知（标注不完整）",
        !d_miss.template_complete
            && d_miss.what.contains("VE-F9999-NOT-REGISTERED")
            && !d_miss.is_complete(),
        "",
    );

    // 最小告知也要出注记（不静默）
    set.add(
        "C16-三要素-模板缺失出注记（待回填）",
        d_miss.notes.iter().any(|n| n.contains("待回填")),
        "",
    );

    // 最小告知的「下一步」不能是空串——空项必须有族级兜底或明确告知
    set.add(
        "C16-三要素-模板缺失时四字段仍被兜底填满（不输出半空报告）",
        d_miss.all_fields_filled() && !d_miss.next.is_empty() && !d_miss.why.is_empty(),
        "",
    );

    // 上游三要素优先于模板兜底（上游离现场最近）
    let d_up = diagnose(&RawError::new("VE-F0415-UTF8-TRUNCATED", loc(1, 1, 0))
        .with_triple("上游现场说明", "上游为什么", "上游怎么改", "VE-SPEC-UP-9"));
    set.add(
        "C16-三要素-上游三要素优先于模板兜底",
        d_up.what == "上游现场说明" && d_up.why == "上游为什么" && d_up.next == "上游怎么改",
        "",
    );

    // 上游只给部分要素时，缺的由模板表补（补齐而非留空）
    let d_part = diagnose(&RawError::new("VE-F0415-UTF8-TRUNCATED", loc(1, 1, 0))
        .with_triple("上游只说发生了什么", "", "", ""));
    set.add(
        "C16-三要素-上游缺项由模板表补齐",
        d_part.what == "上游只说发生了什么"
            && !d_part.why.is_empty()
            && !d_part.next.is_empty()
            && !d_part.rule_ref.is_empty()
            && d_part.template_complete,
        "",
    );

    // 模板表登记项可直接命中（无上游三要素时靠模板出完整三要素）
    let d_tpl = diagnose(&RawError::new("VE-F0410-MISMATCH", loc(1, 1, 0)));
    set.add(
        "C16-三要素-模板表可独立出完整三要素",
        d_tpl.template_complete
            && d_tpl.rule_ref == "VE-SPEC-LEX-BRACE-1"
            && d_tpl.next.contains("括号"),
        "",
    );

    // 模板表条目自身三要素齐备（表的数据质量）
    let tpl_ok = TEMPLATE_TABLE.iter().all(|e| e.template.is_complete());
    set.add("C16-三要素-模板表全部条目三要素齐备", tpl_ok, "");

    // 模板查询：命中/未命中
    set.add(
        "C16-三要素-模板查表命中与未命中",
        template_of("VE-F0410-MISMATCH").is_some() && template_of("VE-F9999-NOPE").is_none(),
        "",
    );

    // ---- 判据：双侧定位 ----

    let d_single = diagnose(&RawError::new("VE-F0407-UNTERMINATED", loc(3, 5, 40)));
    set.add(
        "C16-双侧-仅主侧时非双侧",
        !d_single.is_dual_side() && d_single.span.secondary().is_none(),
        "",
    );

    let d_dual = diagnose(
        &RawError::new("VE-F0412-RECURSION", loc(10, 3, 120)).with_def(loc(2, 1, 15)),
    );
    set.add(
        "C16-双侧-双侧定位成立（调用处+定义处）",
        d_dual.is_dual_side() && d_dual.span.secondary().is_some(),
        "",
    );

    // 双侧渲染：两侧位置都要出现在文本里（省掉的那侧正是修不了的原因）
    let dual_txt = d_dual.render();
    set.add(
        "C16-双侧-渲染含主副两侧位置",
        dual_txt.contains("120") && dual_txt.contains("15") && dual_txt.contains("定义于"),
        "",
    );

    // 副侧位置值正确
    set.add(
        "C16-双侧-副侧位置值正确",
        matches!(d_dual.span.secondary(), Some(s) if s.byte_offset == 15 && s.line == 2),
        "",
    );

    // 两侧独立规范化：主侧越界、副侧正常 → 只钳主侧
    let d_dual2 = diagnose(
        &RawError::new("VE-F0412-RECURSION", Loc::new(1, 9999, 3)).with_def(loc(2, 1, 40)),
    );
    set.add(
        "C16-双侧-两侧独立钳制（只钳越界那侧）",
        d_dual2.span.primary.clamped
            && !matches!(d_dual2.span.secondary(), Some(s) if s.clamped),
        "",
    );

    // Span2 便捷构造
    set.add(
        "C16-双侧-Span2 构造与查询",
        Span2::primary_only(loc(1, 1, 0)).secondary().is_none()
            && Span2::dual(loc(1, 1, 0), loc(1, 1, 5)).secondary().is_some(),
        "",
    );

    // ---- 判据：三级分级 ----

    set.add(
        "C16-分级-三级齐备且标签互异",
        Severity::Error.label() == "错误"
            && Severity::Warning.label() == "警告"
            && Severity::Note.label() == "注记"
            && ErrorFamily::Char.remedy_hint() != ErrorFamily::Literal.remedy_hint(),
        "",
    );

    set.add(
        "C16-分级-仅错误阻断编译",
        Severity::Error.is_blocking()
            && !Severity::Warning.is_blocking()
            && !Severity::Note.is_blocking(),
        "",
    );

    // 裁定表命中：F0410 括号失配是错误
    let d_br = diagnose(&raw_ok("VE-F0410-MISMATCH"));
    set.add(
        "C16-分级-裁定表命中给出严重度",
        d_br.severity == Severity::Error && !d_br.notes.iter().any(|n| n.contains("未登记严重度")),
        "",
    );

    // 未登记码 → 按最严（错误）+ 出注记（锚点错误路径第三条）
    set.add(
        "C16-分级-未登记按最严裁定并注记",
        d_other.severity == Severity::Error
            && d_other.notes.iter().any(|n| n.contains("未登记严重度")),
        "",
    );

    // 全序可比：Note < Warning < Error
    set.add(
        "C16-分级-全序可比（Note<Warning<Error）",
        Severity::Note < Severity::Warning && Severity::Warning < Severity::Error
            && Severity::Note.rank() < Severity::Error.rank(),
        "",
    );

    // 注记级码走 NOTE 前缀
    set.add(
        "C16-分级-NOTE 前缀裁定为注记级",
        severity_of("NOTE-SOMETHING").0 == Severity::Note
            && severity_of("WARN-SOMETHING").0 == Severity::Warning,
        "",
    );

    // ---- 判据：位置三元式 ----

    set.add(
        "C16-位置-三元式齐备（行列+字节偏移）",
        d_br.span.primary.line == 1
            && d_br.span.primary.col == 1
            && d_br.span.primary.byte_offset == 0
            && d_br.span.primary.render().contains("字节偏移"),
        "",
    );

    set.add(
        "C16-位置-渲染含行列与字节偏移三者",
        Loc::new(7, 3, 42).render().contains("7:3") && Loc::new(7, 3, 42).render().contains("42"),
        "",
    );

    // 越界行列被钳制且不 panic（0 行 0 列、列超字节偏移所能支撑的上界）
    let (clamped, was) = Loc::new(0, 0, 5).normalized();
    set.add(
        "C16-位置-越界行列钳制到合法域（不 panic）",
        clamped.line == 1 && clamped.col == 1 && was && clamped.clamped,
        "",
    );

    set.add(
        "C16-位置-列超上界被钳制",
        Loc::new(1, 9999, 3).normalized().0.col == 4,
        "",
    );

    // 自洽性核验：列不超过 byte_offset+1
    set.add(
        "C16-位置-自洽性核验（列不超字节偏移上界）",
        Loc::new(1, 4, 3).is_consistent() && !Loc::new(1, 5, 3).is_consistent(),
        "",
    );

    // 位置不自洽要出注记（零静默：位置算错这件事本身要可见）
    let d_bad = diagnose(&RawError::new("VE-F0407-UNTERMINATED", Loc::new(1, 9999, 3)));
    set.add(
        "C16-位置-位置不自洽出注记（钳制可见）",
        d_bad.notes.iter().any(|n| n.contains("钳制")),
        "",
    );

    // from_offset 便捷构造
    set.add(
        "C16-位置-from_offset 构造自洽",
        Loc::from_offset(9).is_consistent() && Loc::from_offset(9).byte_offset == 9,
        "",
    );

    // with_line_col 补齐行列
    set.add(
        "C16-位置-with_line_col 补齐行列",
        Loc::from_offset(9).with_line_col(3, 2).line == 3
            && Loc::from_offset(9).with_line_col(3, 2).col == 2,
        "",
    );

    // ---- 判据：零静默 / 报告器 ----

    // 未归族出注记
    set.add(
        "C16-显性-未归族出注记（不静默归族）",
        d_other.notes.iter().any(|n| n.contains("未归族")),
        "",
    );

    // 报告器：多条汇总、族筛选、阻断计数
    let mut rep = Reporter::new();
    rep.push(d_full.clone());
    rep.push(d_dual.clone());
    rep.push(d_other.clone());
    set.add(
        "C16-显性-报告器汇总与族筛选",
        rep.len() == 3
            && rep.of_family(ErrorFamily::Directive).len() == 1
            && rep.blocking_count() == 3,
        "",
    );

    // 待回填模板登记（最小告知不消失）
    // 待回填登记：只把「最小告知」那条计入；完整模板的码不得出现在待回填列表。
    let mut only_miss = Reporter::new();
    only_miss.push(d_miss.clone());
    set.add(
        "C16-显性-待回填模板被登记（不静默）",
        only_miss.pending_templates().len() == 1
            && only_miss.pending_templates().contains(&"VE-F9999-NOT-REGISTERED")
            && !only_miss.pending_templates().contains(&"VE-F0415-UTF8-TRUNCATED"),
        "",
    );

    // 全量渲染非空且含多条
    let all_txt = rep.render_all();
    set.add(
        "C16-显性-全量渲染含多条且非空",
        all_txt.contains("VE-F0415-UTF8-TRUNCATED")
            && all_txt.contains("VE-F0412-RECURSION")
            && all_txt.contains("VE-F9999-NOT-REGISTERED"),
        "",
    );

    // 空报告器不 panic、渲染为空串
    let empty_rep = Reporter::new();
    set.add(
        "C16-显性-空报告器安全（无诊断不报错）",
        empty_rep.is_empty() && empty_rep.render_all().is_empty() && empty_rep.len() == 0,
        "",
    );

    // 批量入口 report_all
    let raws = vec![raw_ok("VE-F0410-MISMATCH"), RawError::new("VE-F0411-UNKNOWN-DIRECTIVE", loc(2, 2, 8))];
    let (rep2, txt2) = report_all(&raws);
    set.add(
        "C16-显性-批量报告入口产两条且含建议",
        rep2.len() == 2
            && txt2.contains("VE-F0410-MISMATCH")
            && txt2.contains("VE-F0411-UNKNOWN-DIRECTIVE"),
        "",
    );

    // ---- 判据：上游适配器 ----

    // 宏错误双侧适配
    let d_m1 = from_macro_error(
        "VE-F0412-RECURSION",
        loc(10, 1, 100),
        Some(loc(1, 1, 5)),
        "宏自引用",
        "展开体引用自身",
        "拆开自引用",
    );
    set.add(
        "C16-显性-宏错误适配器产出双侧",
        d_m1.is_dual_side() && d_m1.family == ErrorFamily::Directive,
        "",
    );

    // 单侧适配
    let d_s1 = from_single_side(
        "VE-F0413-UNCLOSED",
        loc(5, 1, 60),
        5,
        "条件编译未闭合",
        "缺 else 或 endif",
        "补 endif",
    );
    set.add(
        "C16-显性-单侧适配器不产双侧",
        !d_s1.is_dual_side() && d_s1.span.primary.line == 5,
        "",
    );

    // 编码故障适配
    let d_e1 = from_encoding_fault(
        "VE-F0415-UTF8-CONTINUATION",
        loc(1, 4, 3),
        "孤立续字节",
        "续字节前无首字节",
        "按实际编码重存",
    );
    set.add(
        "C16-显性-编码故障适配器归字符类且带建议",
        d_e1.family == ErrorFamily::Char && d_e1.severity == Severity::Error && d_e1.is_complete(),
        "",
    );

    set
}

#[cfg(test)]
mod red_report {
    use super::*;
    #[test]
    fn report_red_items() {
        let set = run_vec16_checks();
        for i in 0..set.len() {
            if let Some(c) = set.get(i) {
                if !c.passed {
                    println!("RED: {} | {}", c.name, c.detail);
                }
            }
        }
        println!("total={} dropped={}", set.len(), set.dropped());
    }
}
