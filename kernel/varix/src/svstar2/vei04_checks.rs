//! VE-F4004 · 域自检（判据逐条对应，见 `vei04_typeset.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//!
//! - **四族路由** → `C04-四族-四族齐备`、`C04-四族-每族有语言路由`、
//!   `C04-四族-路由为查表`、`C04-四族-族守卫拒域外`；
//! - **语言覆盖** → `C04-覆盖-未收录不静默`、`C04-覆盖-报告列出缺口`、
//!   `C04-覆盖-收录语言不误标降级`、`C04-覆盖-族性差异影响策略`、
//!   `C04-覆盖-完整标签按子标签命中`、`C04-覆盖-子标签边界不猜族`；
//! - **降级显性** → `C04-降级-落拉丁且标记`、`C04-降级-产诊断`、
//!   `C04-降级-记录带真实语言名`；
//! - **策略执行分工** → `C04-分工-五段唯一归属`、`C04-分工-渲染归执行`、
//!   `C04-分工-对拍失配拒绝`、`C04-分工-计划无执行产物`；
//! - **预留激活** → `C04-预留-未激活不谎报`、`C04-预留-单源唯一`、
//!   `C04-预留-规格表编号连续`、`C04-预留-判据全覆盖`；
//!
//! 另设**反假门禁**组：`C04-反假-断言能失败`、`C04-反假-非ASCII不崩`。
//!
//! `detail` 用 `&'static str`（内核 `CheckSet::add` 只存 `&'static str`），
//! 失败细节由单元测试承担——自检只负责绿/红 + 稳定短码。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vei04_typeset as ts;

/// 自检集构造。
pub fn run_vei04_checks() -> CheckSet {
    let mut set = CheckSet::new(ts::VEA_DOMAIN);
    run(&mut set);
    set
}

/// 一条自检结论。
struct Verdict {
    name: &'static str,
    passed: bool,
    detail: &'static str,
}

impl Verdict {
    fn new(name: &'static str, passed: bool, detail: &'static str) -> Self {
        Verdict { name, passed, detail }
    }
}

/// 全部自检项。
fn run(set: &mut CheckSet) {
    let mut vs: Vec<Verdict> = Vec::new();

    // 判据一：四族路由
    vs.push(route_four_families());
    vs.push(route_each_family_has_language());
    vs.push(route_table_lookup());
    vs.push(route_family_guard_rejects());

    // 判据二：语言覆盖
    vs.push(coverage_no_silent_default());
    vs.push(coverage_report_lists_gaps());
    vs.push(coverage_known_not_degraded());
    vs.push(coverage_family_affects_policy());
    vs.push(coverage_full_tag_hits_primary_subtag());
    vs.push(coverage_primary_subtag_edges());

    // 判据三：降级显性
    vs.push(degrade_to_latin_marked());
    vs.push(degrade_diagnostic_emitted());
    vs.push(degrade_record_real_language());

    // 判据四：策略执行分工
    vs.push(split_five_stages());
    vs.push(render_execution_only());
    vs.push(stage_handoff_checked());
    vs.push(plan_has_no_execution_output());

    // 判据五：预留激活
    vs.push(reserved_not_lied());
    vs.push(single_source_unique());
    vs.push(spec_numbering_continuous());
    vs.push(criteria_covered());

    // 反假门禁
    vs.push(assertions_can_fail());
    vs.push(non_ascii_no_crash());

    for v in vs {
        set.add(v.name, v.passed, if v.passed { "" } else { v.detail });
    }
}

// ---------------------------------------------------------------------------
// 判据一：四族路由
// ---------------------------------------------------------------------------

fn route_four_families() -> Verdict {
    let all = ts::ScriptFamily::ALL;
    let mut shorts: Vec<&str> = all.iter().map(|f| f.short()).collect();
    shorts.sort_unstable();
    shorts.dedup();
    let ok = all.len() == 4
        && shorts.len() == 4
        && all.iter().all(|f| !f.name().is_empty() && !f.short().is_empty());
    Verdict::new("C04-四族-四族齐备", ok, "self-check-fail")
}

fn route_each_family_has_language() -> Verdict {
    // 四族每族至少一条路由（否则某族无策略可用）。
    let mut ok = true;
    for f in ts::ScriptFamily::ALL {
        if !ts::LANGUAGE_TABLE.iter().any(|r| r.family == f) {
            ok = false;
            break;
        }
    }
    // 表内语言标签不得重复（重复会让先命中的赢，后面的永远不可达）。
    let mut langs: Vec<&str> = ts::LANGUAGE_TABLE.iter().map(|r| r.language).collect();
    let before = langs.len();
    langs.sort_unstable();
    langs.dedup();
    if langs.len() != before {
        ok = false;
    }
    Verdict::new("C04-四族-每族有语言路由", ok, "self-check-fail")
}

fn route_table_lookup() -> Verdict {
    // 路由是查表：表内语言全命中且不降级；表外语言必降级。
    let mut ok = true;
    for e in ts::LANGUAGE_TABLE.iter() {
        let r = ts::route(e.language);
        if r.degraded || r.family != e.family || r.rtl != e.rtl {
            ok = false;
            break;
        }
    }
    Verdict::new("C04-四族-路由为查表", ok, "self-check-fail")
}

fn route_family_guard_rejects() -> Verdict {
    let mut ok = true;
    for (raw, want) in [
        ("Latin", ts::ScriptFamily::Latin),
        ("cjk", ts::ScriptFamily::Cjk),
        ("Arabic", ts::ScriptFamily::Arabic),
        ("TAIINDIC", ts::ScriptFamily::TaiIndic),
    ] {
        match ts::guard_family(raw) {
            Ok(got) if got == want => {}
            _ => {
                ok = false;
                break;
            }
        }
    }
    for bad in ["", "Klingon", "LATIN1", "汉"] {
        match ts::guard_family(bad) {
            Ok(_) => {
                ok = false;
                break;
            }
            Err(e) => {
                if e.code != ts::E_FAMILY_INVALID || !e.is_complete() {
                    ok = false;
                    break;
                }
            }
        }
    }
    Verdict::new("C04-四族-族守卫拒域外", ok, "self-check-fail")
}

// ---------------------------------------------------------------------------
// 判据二：语言覆盖
// ---------------------------------------------------------------------------

fn coverage_no_silent_default() -> Verdict {
    // 表外语言一律降级 + 标记，不静默走默认族。
    let mut ok = true;
    for lang in ["sw", "zu", "am", "yue", "eo", "xx"] {
        let r = ts::route(lang);
        if !r.degraded || r.degradation.is_none() {
            ok = false;
            break;
        }
        if r.family != ts::FALLBACK_FAMILY {
            ok = false;
            break;
        }
    }
    Verdict::new("C04-覆盖-未收录不静默", ok, "self-check-fail")
}

fn coverage_report_lists_gaps() -> Verdict {
    // 覆盖报告须挑出缺口语言，且去重。
    let r = ts::LanguageCoverageReport::probe(&["en", "zh", "sw", "sw", "zu", "ar"]);
    let ok = r.has_gap
        && r.uncovered.len() == 2
        && r.covered == 3
        && r.coverage_percent() < 100
        && r.coverage_percent() > 0;
    Verdict::new("C04-覆盖-报告列出缺口", ok, "self-check-fail")
}

fn coverage_known_not_degraded() -> Verdict {
    // 表内语言不得被误标降级（误标会让上层无谓报警）。
    let mut ok = true;
    for e in ts::LANGUAGE_TABLE.iter() {
        if ts::route(e.language).degraded {
            ok = false;
            break;
        }
    }
    // RTL 族方向标记正确。
    if !ts::route("ar").rtl || ts::route("en").rtl {
        ok = false;
    }
    Verdict::new("C04-覆盖-收录语言不误标降级", ok, "self-check-fail")
}

fn coverage_family_affects_policy() -> Verdict {
    // 四族的策略参数必须**真的不同**——若四族策略一致，
    // 说明"分四族"只是标签，实际没起作用（那等于没做路由）。
    let lat = ts::policy_for(ts::ScriptFamily::Latin, false);
    let cjk = ts::policy_for(ts::ScriptFamily::Cjk, false);
    let ara = ts::policy_for(ts::ScriptFamily::Arabic, true);
    let tai = ts::policy_for(ts::ScriptFamily::TaiIndic, false);
    // CJK 独有禁则。
    if !cjk.line_break_rules || lat.line_break_rules {
        return Verdict::new("C04-覆盖-族性差异影响策略", false, "self-check-fail");
    }
    // 簇族按簇分词 + 需整形 + 基线对齐。
    if !ara.tokenize_by_cluster
        || !tai.tokenize_by_cluster
        || lat.tokenize_by_cluster
        || !ara.shape_required
        || !tai.shape_required
        || !ara.baseline_align
    {
        return Verdict::new("C04-覆盖-族性差异影响策略", false, "self-check-fail");
    }
    // RTL 镜像。
    if !ara.mirror || lat.mirror {
        return Verdict::new("C04-覆盖-族性差异影响策略", false, "self-check-fail");
    }
    Verdict::new("C04-覆盖-族性差异影响策略", true, "")
}

/// 完整 BCP47 标签必须能路由（锚点「全语言覆盖」的本域落点）。
///
/// 这项是**为堵一个真缺陷而设**：原`route()` 只按整串查表，而调用方交的是
/// F4002 规范化后的完整标签（`zh-Hans-CN` / `ar-EG` / `ta-IN`），于是每个带
/// 区域/脚本子标签的语言都被误报"未收录"——覆盖报告塞满假缺口。
/// 这里逐族各取一个真实标签，断言**族与方向都与裸码一致且不降级**。
fn coverage_full_tag_hits_primary_subtag() -> Verdict {
    let cases: [(&str, ts::ScriptFamily, bool); 6] = [
        ("en-US", ts::ScriptFamily::Latin, false),
        ("zh-Hans-CN", ts::ScriptFamily::Cjk, false),
        ("ja-JP", ts::ScriptFamily::Cjk, false),
        ("ar-EG", ts::ScriptFamily::Arabic, true),
        ("ta-IN", ts::ScriptFamily::TaiIndic, false),
        ("de-DE-u-co-phonebk", ts::ScriptFamily::Latin, false),
    ];
    for (tag, want_family, want_rtl) in cases.iter() {
        let r = ts::route(tag);
        if r.degraded
            || r.family != *want_family
            || r.rtl != *want_rtl
            || r.matched != ts::RouteMatch::PrimarySubtag
        {
            return Verdict::new("C04-覆盖-完整标签按子标签命中", false, "self-check-fail");
        }
    }
    // 裸码必须走 Exact 层级（两级查表不能把裸码也标成子标签命中，
    // 否则命中层级失去追因价值）。
    if ts::route("ar").matched != ts::RouteMatch::Exact {
        return Verdict::new("C04-覆盖-完整标签按子标签命中", false, "self-check-fail");
    }
    Verdict::new("C04-覆盖-完整标签按子标签命中", true, "")
}

/// 主语言子标签抽取的边界（畸形标签不得panic、不得被猜族）。
///
/// 覆盖：空串、以 `-` 开头、`und`、`x-` 私有用途、通配 `*`。
/// 这些都**必须落空降级**而不是猜一个族——猜族就是降级红线禁止的静默行为。
fn coverage_primary_subtag_edges() -> Verdict {
    if ts::primary_subtag("zh-Hans-CN") != "zh" {
        return Verdict::new("C04-覆盖-子标签边界不猜族", false, "self-check-fail");
    }
    if ts::primary_subtag("ar") != "ar" {
        return Verdict::new("C04-覆盖-子标签边界不猜族", false, "self-check-fail");
    }
    if !ts::primary_subtag("").is_empty() {
        return Verdict::new("C04-覆盖-子标签边界不猜族", false, "self-check-fail");
    }
    if !ts::primary_subtag("-CN").is_empty() {
        return Verdict::new("C04-覆盖-子标签边界不猜族", false, "self-check-fail");
    }
    // 这些一律 Missed + 显性降级，不得静默给族。
    for tag in ["", "-CN", "und", "und-CN", "x-private", "*", "zz-ZZ", "汉"] {
        let r = ts::route(tag);
        if !r.degraded
            || r.matched != ts::RouteMatch::Missed
            || r.degradation.is_none()
            || r.family != ts::FALLBACK_FAMILY
        {
            return Verdict::new("C04-覆盖-子标签边界不猜族", false, "self-check-fail");
        }
        // 降级记录必须带真实传入名（不能是占位符）。
        let d = r.degradation.unwrap();
        if d.language != tag {
            return Verdict::new("C04-覆盖-子标签边界不猜族", false, "self-check-fail");
        }
    }
    Verdict::new("C04-覆盖-子标签边界不猜族", true, "")
}

// ---------------------------------------------------------------------------
// 判据三：降级显性
// ---------------------------------------------------------------------------

fn degrade_to_latin_marked() -> Verdict {
    // 降级必落拉丁族且带标记。
    let mut ok = true;
    for lang in ["sw", "tl", "mi"] {
        let r = ts::route(lang);
        match r.degradation {
            Some(d) if d.fell_back_to == ts::FALLBACK_FAMILY && d.language == lang => {}
            _ => {
                ok = false;
                break;
            }
        }
    }
    Verdict::new("C04-降级-落拉丁且标记", ok, "self-check-fail")
}

fn degrade_diagnostic_emitted() -> Verdict {
    // 降级必产诊断（红线实测：静默降级是最坏做法）。
    let mut ok = true;
    for lang in ["sw", "zu"] {
        let p = match ts::plan(lang, "some text here") {
            Ok(p) => p,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if !p.degraded() || p.bag.count_of(ts::DiagKind::DegradedToLatin) != 1 {
            ok = false;
            break;
        }
        // 诊断内容须点名该语言（否则报警时不知道是谁）。
        let named = p
            .bag
            .items()
            .iter()
            .any(|d| d.kind == ts::DiagKind::DegradedToLatin && d.what.contains(lang));
        if !named {
            ok = false;
            break;
        }
    }
    // 已收录语言不得产降级诊断。
    let p2 = ts::plan("en", "hello").unwrap();
    if p2.bag.count_of(ts::DiagKind::DegradedToLatin) != 0 {
        ok = false;
    }
    Verdict::new("C04-降级-产诊断", ok, "self-check-fail")
}

fn degrade_record_real_language() -> Verdict {
    // 降级记录带真实语言名（不留占位符）。
    let d = ts::route("sw").degradation.unwrap();
    let ok = d.language == "sw" && !d.language.is_empty() && d.language != "und";
    Verdict::new("C04-降级-记录带真实语言名", ok, "self-check-fail")
}

// ---------------------------------------------------------------------------
// 判据四：策略执行分工
// ---------------------------------------------------------------------------

fn split_five_stages() -> Verdict {
    // 分工表机检 + 五段齐备 + 陈述非空。
    let machine = ts::check_execution_split().is_ok();
    let mut ok = machine;
    for st in ts::Stage::ALL {
        let n = ts::SPLIT_TABLE.iter().filter(|s| s.stage == st).count();
        if n != 1 {
            ok = false;
            break;
        }
        let item = ts::SPLIT_TABLE.iter().find(|s| s.stage == st).unwrap();
        if item.statement.is_empty() {
            ok = false;
            break;
        }
    }
    Verdict::new("C04-分工-五段唯一归属", ok, "self-check-fail")
}

fn render_execution_only() -> Verdict {
    // 渲染必须完全归执行侧（本项不产出绘制结果，硬边界）。
    let r = ts::SPLIT_TABLE
        .iter()
        .find(|s| s.stage == ts::Stage::Render)
        .unwrap();
    let ok = r.side == ts::ExecutionSplit::Execution;
    Verdict::new("C04-分工-渲染归执行", ok, "self-check-fail")
}

fn stage_handoff_checked() -> Verdict {
    // 段间对拍：一致过、不一致拒（且三要素齐）。
    let d = ts::StageDigest { items: 5, covered_bytes: 42 };
    let ok = ts::check_stage_handoff(ts::Stage::Tokenize, d.clone(), 5).is_ok()
        && match ts::check_stage_handoff(ts::Stage::Render, d, 6) {
            Err(e) => e.code == ts::E_STAGE_MISMATCH && e.is_complete(),
            Ok(_) => false,
        };
    Verdict::new("C04-分工-对拍失配拒绝", ok, "self-check-fail")
}

fn plan_has_no_execution_output() -> Verdict {
    // 计划只含策略与计数，零隐私面机检过。
    let mut ok = true;
    for lang in ["en", "zh", "ar", "hi"] {
        let p = match ts::plan(lang, "sample text内容 مثال") {
            Ok(p) => p,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if ts::check_zero_privacy(&p).is_err() {
            ok = false;
            break;
        }
        // 摘要只含计数与字节数（不存正文）。
        if p.tokenized.covered_bytes == 0 && p.lines.covered_bytes == 0 {
            ok = false;
            break;
        }
    }
    Verdict::new("C04-分工-计划无执行产物", ok, "self-check-fail")
}

// ---------------------------------------------------------------------------
// 判据五：预留激活
// ---------------------------------------------------------------------------

fn reserved_not_lied() -> Verdict {
    // F2910/F2915/F4041/F4047 未实现 → 槽位必须 landed=false。
    let machine = ts::check_reserved_slots().is_ok();
    let mut all_unlanded = true;
    for slot in ts::RESERVED_SLOTS {
        if slot.landed {
            all_unlanded = false;
        }
    }
    let covered = ts::RESERVED_SLOTS.len() == 4;
    Verdict::new(
        "C04-预留-未激活不谎报",
        machine && all_unlanded && covered,
        "self-check-fail",
    )
}

fn single_source_unique() -> Verdict {
    let ok = ts::check_single_source().is_ok();
    // 路由与策略的 owner 必须是本项（被N02 抢走 owner = 双份路由表）。
    let mut route_owned = false;
    let mut policy_owned = false;
    for c in ts::TYPESET_SINGLE_SOURCE {
        if c.key == "typeset-family-route" {
            route_owned = c.owner == "VE-F4004";
        }
        if c.key == "typeset-stage-policy" {
            policy_owned = c.owner == "VE-F4004";
        }
        // 方向取值必须仍归 F4003（单向依赖，本项不抢）。
        if c.key == "typeset-direction-source" && c.owner != "VE-F4003" {
            return Verdict::new("C04-预留-单源唯一", false, "self-check-fail");
        }
    }
    Verdict::new(
        "C04-预留-单源唯一",
        ok && route_owned && policy_owned,
        "self-check-fail",
    )
}

fn spec_numbering_continuous() -> Verdict {
    let machine = ts::check_spec_coverage().is_ok();
    let mut nums: Vec<u16> = ts::SPEC_SHEET.iter().map(|s| s.no).collect();
    nums.sort_unstable();
    let continuous = nums
        .iter()
        .enumerate()
        .all(|(i, v)| *v == (i + 1) as u16);
    let labeled = ts::SPEC_SHEET
        .iter()
        .all(|s| !s.key.is_empty() && !s.label.is_empty() && !s.enforced_by.is_empty());
    let ok = machine && continuous && labeled && ts::SPEC_SHEET.len() >= 13;
    Verdict::new("C04-预留-规格表编号连续", ok, "self-check-fail")
}

fn criteria_covered() -> Verdict {
    let machine = ts::check_spec_coverage().is_ok();
    let all_mapped = ts::CRITERIA.len() == 5
        && ts::CRITERIA
            .iter()
            .all(|(_, keys)| keys.iter().all(|k| ts::SPEC_SHEET.iter().any(|s| s.key == *k)));
    Verdict::new(
        "C04-预留-判据全覆盖",
        machine && all_mapped,
        "self-check-fail",
    )
}

// ---------------------------------------------------------------------------
// 反假门禁
// ---------------------------------------------------------------------------

fn assertions_can_fail() -> Verdict {
    // 证明守卫不是恒真的：拿故意错误的输入撞，断言必须被拒。
    let family = ts::guard_family("Klingon").is_err();
    let stage = ts::guard_stage("rasterize").is_err();
    let handoff = ts::check_stage_handoff(
        ts::Stage::Tokenize,
        ts::StageDigest { items: 1, covered_bytes: 1 },
        99,
    )
    .is_err();
    let oversize = ts::plan("en", &"x".repeat(ts::MAX_TEXT_LEN + 1)).is_err();
    let ok = family && stage && handoff && oversize;
    Verdict::new("C04-反假-断言能失败", ok, "self-check-fail")
}

fn non_ascii_no_crash() -> Verdict {
    // 反假门禁：非 ASCII 文本全链路不 panic（F4003 踩过 UTF-8 边界，
    // 本项经 `chars().count()` 与 `split_whitespace()`，同样要钉住）。
    let mut ok = true;
    for t in [
        "今天天气不错",
        "مرحبا بالعالم",
        "こんにちは世界",
        "สวัสดีชาวโลก",
        "नमस्ते दुनिया",
        "mixed 混合 مixed",
    ] {
        for lang in ["zh", "ar", "ja", "th", "hi", "en"] {
            if ts::plan(lang, t).is_err() {
                ok = false;
                break;
            }
        }
        if !ok {
            break;
        }
    }
    Verdict::new("C04-反假-非ASCII不崩", ok, "self-check-fail")
}