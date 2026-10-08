//! VE-F4006 · 域自检（判据逐条对应，见 `vei06_datetime.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//!
//! - **CLDR 锚定** → `C06-cldr-版本为编译期常量`、`C06-cldr-und兜底必存在`、
//!   `C06-cldr-规则集非空`；
//! - **五类格式** → `C06-五类-闭域齐备`、`C06-五类-守卫拒域外`、
//!   `C06-五类-渲染互不相同`、`C06-五类-未知locale回退und`、
//!   `C06-五类-完整标签按子标签命中`；
//! - **多历法** → `C06-历法-四系齐备`、`C06-历法-纪元公历可逆`、
//!   `C06-历法-纪元越界拒绝`、`C06-历法-近似必须显式`；
//! - **时区断言** → `C06-时区-非法一律断言`、`C06-时区-偏移钳制可见`、
//!   `C06-时区-本地时刻真的偏移`；
//! - **热表单源** → `C06-缓存-键含历法时区类别小数位`、`C06-缓存-键含cldr版本`、
//!   `C06-单源-热表owner唯一`、`C06-预留-未落地不谎报`、
//!   `C06-预留-规格编号连续`、`C06-预留-判据全覆盖`；
//!
//! 另设**反假门禁**组：`C06-反假-断言能失败`、`C06-反假-非ASCII不崩`、
//! `C06-反假-五类真不同`。
//!
//! `detail` 用 `&'static str`（内核 `CheckSet::add` 只存 `&'static str`），
//! 失败细节由单元测试承担——自检只负责绿/红 + 稳定短码。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vei06_datetime as dt;

/// 自检集构造。
pub fn run_vei06_checks() -> CheckSet {
    let mut set = CheckSet::new(dt::VEA_DOMAIN);
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

    // 判据一：CLDR 锚定
    vs.push(cldr_version_pinned());
    vs.push(cldr_und_fallback());
    vs.push(cldr_rules_non_empty());
    vs.push(locale_coverage_aligned());

    // 判据二：五类格式
    vs.push(five_kinds_closed());
    vs.push(five_kinds_guard_rejects());
    vs.push(five_kinds_render_differently());
    vs.push(locale_fallback_und());
    vs.push(locale_two_level_lookup());

    // 判据三：多历法
    vs.push(four_calendars());
    vs.push(epoch_roundtrip());
    vs.push(epoch_out_of_range());
    vs.push(calendar_approx_visible());

    // 判据四：时区断言
    vs.push(tz_asserted());
    vs.push(tz_clamped_visible());
    vs.push(tz_shifts_local_time());

    // 判据五：热表单源
    vs.push(cache_key_complete());
    vs.push(cache_key_has_cldr_version());
    vs.push(hot_table_single_source());
    vs.push(reserved_not_lied());
    vs.push(spec_numbering_continuous());
    vs.push(criteria_covered());

    // 反假门禁
    vs.push(assertions_can_fail());
    vs.push(non_ascii_no_crash());
    vs.push(five_kinds_truly_differ());

    for v in vs {
        set.add(v.name, v.passed, if v.passed { "" } else { v.detail });
    }
}

// ---------------------------------------------------------------------------
// 判据一：CLDR 锚定
// ---------------------------------------------------------------------------

fn cldr_version_pinned() -> Verdict {
    // 锚定版本必须是**非零编译期常量**且与 tag 一致。
    // 若为 0，说明有人把锚定删了——那时规则集可能已经漂到别的版本上。
    if dt::CLDR_VERSION == 0 {
        return Verdict::new("C06-cldr-版本为编译期常量", false, "self-check-fail");
    }
    let expect = format!("cldr-{}", dt::CLDR_VERSION);
    if dt::CLDR_VERSION_TAG != expect {
        return Verdict::new("C06-cldr-版本为编译期常量", false, "self-check-fail");
    }
    Verdict::new("C06-cldr-版本为编译期常量", true, "")
}

fn cldr_und_fallback() -> Verdict {
    // 规则集**必须**含 und 兜底条目——缺了它，未收录 Locale 无处可退，
    // 降级就会退化成崩溃。
    match dt::check_cldr_anchor() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C06-cldr-und兜底必存在", false, "self-check-fail"),
    }
    let und = match dt::lookup_rule("und") {
        // 注意：这里查的是**表里那一行 und**，不是"查不到时的兜底"（那是另一个分支）。
        dt::RuleLookup::Exact(r) => *r,
        _ => return Verdict::new("C06-cldr-und兜底必存在", false, "self-check-fail"),
    };
    // und 必须是**西式风格**（全球可读）。
    if und.decimal_sep != '.' || und.group_size != 3 {
        return Verdict::new("C06-cldr-und兜底必存在", false, "self-check-fail");
    }
    Verdict::new("C06-cldr-und兜底必存在", true, "")
}

fn cldr_rules_non_empty() -> Verdict {
    // 规则集非空，且每条规则的 locale 非空、分组大小合理。
    if dt::CLDR_RULES.is_empty() {
        return Verdict::new("C06-cldr-规则集非空", false, "self-check-fail");
    }
    for r in dt::CLDR_RULES.iter() {
        if r.locale.is_empty() || r.currency_symbol.is_empty() {
            return Verdict::new("C06-cldr-规则集非空", false, "self-check-fail");
        }
        if r.group_size > 4 {
            return Verdict::new("C06-cldr-规则集非空", false, "self-check-fail");
        }
    }
    // locale 不得重复（重复会让先命中的赢，后面的永不可达）。
    let mut locs: Vec<&str> = dt::CLDR_RULES.iter().map(|r| r.locale).collect();
    let before = locs.len();
    locs.sort_unstable();
    locs.dedup();
    if locs.len() != before {
        return Verdict::new("C06-cldr-规则集非空", false, "self-check-fail");
    }
    Verdict::new("C06-cldr-规则集非空", true, "")
}

fn locale_coverage_aligned() -> Verdict {
    // **域间对齐门禁**：字体域（F4004）声明支持的语言，本域必须能正确排版。
    //
    // 这项是实测缺陷的守卫：初版规则表只 12 条，F4004 支持 25 种，
    // 泰米尔语 `ta` 落在缺口里——字体域选对了本地字体，日期域却回退 und
    // 按 ISO 排。页面"看起来排上了"（不是方框），但日期格式是错的，
    // 而用户完全看不出发生了什么。
    match dt::check_locale_coverage_alignment() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C06-cldr-覆盖与字体域对齐", false, "self-check-fail"),
    }
    // 逐条反查：对齐清单里的每个语言都必须能查到规则，且不是 und。
    for lang in dt::ALIGNED_LANGUAGES.iter() {
        let lk = dt::lookup_rule(lang);
        if lk.is_missed() || lk.rule().locale != *lang {
            return Verdict::new("C06-cldr-覆盖与字体域对齐", false, "self-check-fail");
        }
    }
    // 反向也成立：查已收录语言不得回退 und（防"对齐清单写了但规则是 und"）。
    for lang in dt::ALIGNED_LANGUAGES.iter() {
        if dt::lookup_rule(lang).rule().locale == "und" {
            return Verdict::new("C06-cldr-覆盖与字体域对齐", false, "self-check-fail");
        }
    }
    Verdict::new("C06-cldr-覆盖与字体域对齐", true, "")
}

// ---------------------------------------------------------------------------
// 判据二：五类格式
// ---------------------------------------------------------------------------

fn five_kinds_closed() -> Verdict {
    // 五类齐备（锚点钦定闭域），且每类有稳定名、命名唯一。
    if dt::FormatKind::ALL.len() != 5 {
        return Verdict::new("C06-五类-闭域齐备", false, "self-check-fail");
    }
    let mut names: Vec<&str> = dt::FormatKind::ALL.iter().map(|k| k.name()).collect();
    let before = names.len();
    names.sort_unstable();
    names.dedup();
    if names.len() != before {
        return Verdict::new("C06-五类-闭域齐备", false, "self-check-fail");
    }
    // 数值类恰三类（数字/货币/百分比），日期时间两类。
    let numeric = dt::FormatKind::ALL.iter().filter(|k| k.is_numeric()).count();
    if numeric != 3 {
        return Verdict::new("C06-五类-闭域齐备", false, "self-check-fail");
    }
    Verdict::new("C06-五类-闭域齐备", true, "")
}

fn five_kinds_guard_rejects() -> Verdict {
    // 守卫收全大小写形态，但**域外值必须拒**。
    for (raw, want) in [
        ("date", dt::FormatKind::Date),
        ("TIME", dt::FormatKind::Time),
        ("Number", dt::FormatKind::Number),
        ("CURRENCY", dt::FormatKind::Currency),
        ("Pct", dt::FormatKind::Percent),
    ] {
        match dt::FormatKind::guard(raw) {
            Ok(g) if g == want => {}
            _ => return Verdict::new("C06-五类-守卫拒域外", false, "self-check-fail"),
        }
    }
    for bad in ["", "datetime", "int", "CURENCYY", "汉"] {
        match dt::FormatKind::guard(bad) {
            Ok(_) => return Verdict::new("C06-五类-守卫拒域外", false, "self-check-fail"),
            Err(e) => {
                if e.code != dt::E_KIND_INVALID || !e.is_complete() {
                    return Verdict::new("C06-五类-守卫拒域外", false, "self-check-fail");
                }
            }
        }
    }
    Verdict::new("C06-五类-守卫拒域外", true, "")
}

fn five_kinds_render_differently() -> Verdict {
    // 五类必须**真的渲染出不同东西**——若一致，"分五类"只是标签。
    let base = 1704067200i64;
    let mut texts: [String; 5] = [String::new(), String::new(), String::new(), String::new(), String::new()];
    for (i, k) in dt::FormatKind::ALL.iter().enumerate() {
        match dt::format(&dt::FormatRequest::new(
            "en-US",
            *k,
            base,
            dt::CalendarSystem::Gregorian,
            dt::TimeZone::UTC,
            2,
        )) {
            Ok(r) => texts[i] = r.text,
            Err(_) => return Verdict::new("C06-五类-渲染互不相同", false, "self-check-fail"),
        }
    }
    // 两两比较必须全不同（5 项 = 10 对）。
    for i in 0..texts.len() {
        for j in (i + 1)..texts.len() {
            if texts[i] == texts[j] {
                return Verdict::new("C06-五类-渲染互不相同", false, "self-check-fail");
            }
        }
    }
    // 各自的形态特征：时间带冒号、货币带符号、百分比带百分号。
    let time = texts[1].clone();
    if !time.contains(':') {
        return Verdict::new("C06-五类-渲染互不相同", false, "self-check-fail");
    }
    let cur = texts[3].clone();
    if !cur.starts_with('$') {
        return Verdict::new("C06-五类-渲染互不相同", false, "self-check-fail");
    }
    let pct = texts[4].clone();
    if !pct.ends_with('%') {
        return Verdict::new("C06-五类-渲染互不相同", false, "self-check-fail");
    }
    Verdict::new("C06-五类-渲染互不相同", true, "")
}

fn locale_fallback_und() -> Verdict {
    // 未收录 Locale → und 格式 + 显式标记 + 诊断（不静默）。
    let r = match dt::format(&dt::FormatRequest::new(
        "sw-KE",
        dt::FormatKind::Date,
        1704067200,
        dt::CalendarSystem::Gregorian,
        dt::TimeZone::UTC,
        0,
    )) {
        Ok(x) => x,
        Err(_) => return Verdict::new("C06-五类-未知locale回退und", false, "self-check-fail"),
    };
    if !r.degraded || r.effective_locale != "und" || r.rule_match != "missed" {
        return Verdict::new("C06-五类-未知locale回退und", false, "self-check-fail");
    }
    if r.bag.count_of(dt::DiagKind::UndedLocale) < 1 {
        return Verdict::new("C06-五类-未知locale回退und", false, "self-check-fail");
    }
    // 已收录不得误标降级（误标会让上层无谓报警）。
    match dt::format(&dt::FormatRequest::new(
        "de-DE",
        dt::FormatKind::Date,
        1704067200,
        dt::CalendarSystem::Gregorian,
        dt::TimeZone::UTC,
        0,
    )) {
        Ok(x) => {
            if x.degraded || x.bag.count_of(dt::DiagKind::UndedLocale) != 0 {
                return Verdict::new("C06-五类-未知locale回退und", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C06-五类-未知locale回退und", false, "self-check-fail"),
    }
    Verdict::new("C06-五类-未知locale回退und", true, "")
}

fn locale_two_level_lookup() -> Verdict {
    // 完整 BCP47 标签必须按主语言子标签命中（否则每个带区域标签的语言都被
    // 误报"未收录"，覆盖报告堆满假缺口——这正是 F4004 修过的同类缺陷）。
    for (tag, want_primary) in [
        ("de-DE", true),
        ("zh-Hans-CN", true),
        ("ar-EG", true),
        ("en-US", true),
        ("de", false),
    ] {
        let lk = dt::lookup_rule(tag);
        if lk.is_missed() {
            return Verdict::new("C06-五类-完整标签按子标签命中", false, "self-check-fail");
        }
        let is_primary = lk.matched() == "primary";
        if is_primary != want_primary {
            return Verdict::new("C06-五类-完整标签按子标签命中", false, "self-check-fail");
        }
    }
    // 子标签抽取边界：畸形不得panic、不得猜族。
    if dt::primary_subtag("zh-Hans-CN") != "zh" {
        return Verdict::new("C06-五类-完整标签按子标签命中", false, "self-check-fail");
    }
    if !dt::primary_subtag("").is_empty() || !dt::primary_subtag("-CN").is_empty() {
        return Verdict::new("C06-五类-完整标签按子标签命中", false, "self-check-fail");
    }
    Verdict::new("C06-五类-完整标签按子标签命中", true, "")
}

// ---------------------------------------------------------------------------
// 判据三：多历法
// ---------------------------------------------------------------------------

fn four_calendars() -> Verdict {
    // 四历法齐备 + 守卫收全大小写（含别名）+ 域外拒。
    if dt::CalendarSystem::ALL.len() != 4 {
        return Verdict::new("C06-历法-四系齐备", false, "self-check-fail");
    }
    for (raw, want) in [
        ("gregorian", dt::CalendarSystem::Gregorian),
        ("ISO", dt::CalendarSystem::Gregorian),
        ("Hebrew", dt::CalendarSystem::Hebrew),
        ("BUDDHIST", dt::CalendarSystem::Buddhist),
        ("hijri", dt::CalendarSystem::Islamic),
    ] {
        match dt::CalendarSystem::guard(raw) {
            Ok(g) if g == want => {}
            _ => return Verdict::new("C06-历法-四系齐备", false, "self-check-fail"),
        }
    }
    for bad in ["", "julian", "chinese", "汉"] {
        match dt::CalendarSystem::guard(bad) {
            Ok(_) => return Verdict::new("C06-历法-四系齐备", false, "self-check-fail"),
            Err(e) => {
                if e.code != dt::E_CALENDAR_INVALID || !e.is_complete() {
                    return Verdict::new("C06-历法-四系齐备", false, "self-check-fail");
                }
            }
        }
    }
    Verdict::new("C06-历法-四系齐备", true, "")
}

fn epoch_roundtrip() -> Verdict {
    // 纪元↔公历必须可逆（含负纪元与闰年 2 月 29 日）。
    for (epoch, y, m, d) in [
        (0i64, 1970i64, 1u32, 1u32),
        (1704067200, 2024, 1, 1),
        (951782400, 2000, 2, 29),
        (dt::MIN_EPOCH_SEC, 1900, 1, 1),
    ] {
        let c = match dt::civil_from_epoch(epoch) {
            Ok(x) => x,
            Err(_) => return Verdict::new("C06-历法-纪元公历可逆", false, "self-check-fail"),
        };
        if (c.year, c.month, c.day) != (y, m, d) {
            return Verdict::new("C06-历法-纪元公历可逆", false, "self-check-fail");
        }
        match dt::epoch_from_civil(&c) {
            Ok(back) if back == epoch => {}
            _ => return Verdict::new("C06-历法-纪元公历可逆", false, "self-check-fail"),
        }
    }
    // 闰年判定本身也要对。
    if !dt::is_gregorian_leap(2000) || dt::is_gregorian_leap(1900) || !dt::is_gregorian_leap(2024) {
        return Verdict::new("C06-历法-纪元公历可逆", false, "self-check-fail");
    }
    Verdict::new("C06-历法-纪元公历可逆", true, "")
}

fn epoch_out_of_range() -> Verdict {
    // 纪元越界显性拒绝（不静默算出一个垃圾日期）。
    for bad in [dt::MIN_EPOCH_SEC - 1, dt::MAX_EPOCH_SEC + 1] {
        match dt::civil_from_epoch(bad) {
            Ok(_) => return Verdict::new("C06-历法-纪元越界拒绝", false, "self-check-fail"),
            Err(e) => {
                if e.code != dt::E_EPOCH_OUT_OF_RANGE || !e.is_complete() {
                    return Verdict::new("C06-历法-纪元越界拒绝", false, "self-check-fail");
                }
            }
        }
    }
    // 日越界也必须拒（2 月 30 日）。
    let bad_day = dt::CivilDateTime {
        year: 2023,
        month: 2,
        day: 30,
        hour: 0,
        minute: 0,
        second: 0,
    };
    if dt::epoch_from_civil(&bad_day).is_ok() {
        return Verdict::new("C06-历法-纪元越界拒绝", false, "self-check-fail");
    }
    Verdict::new("C06-历法-纪元越界拒绝", true, "")
}

fn calendar_approx_visible() -> Verdict {
    // 近似历法必须**显式**标approx 并产诊断（不静默给个像模像样的错日期）。
    if !dt::is_approximate(dt::CalendarSystem::Islamic)
        || !dt::is_approximate(dt::CalendarSystem::Hebrew)
    {
        return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail");
    }
    // 公历与佛历是精确的（佛历 = 公历 + 543，固定偏移）。
    if dt::is_approximate(dt::CalendarSystem::Gregorian)
        || dt::is_approximate(dt::CalendarSystem::Buddhist)
    {
        return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail");
    }
    match dt::format(&dt::FormatRequest::new(
        "ar-EG",
        dt::FormatKind::Date,
        1704067200,
        dt::CalendarSystem::Islamic,
        dt::TimeZone::UTC,
        0,
    )) {
        Ok(r) => {
            if !r.approx || r.bag.count_of(dt::DiagKind::CalendarApproximate) < 1 {
                return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail");
            }
            if r.calendar_date.map(|c| c.era) != Some("AH") {
                return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail"),
    }
    // 佛历年份必须是公历 + 543。
    match dt::format(&dt::FormatRequest::new(
        "th-TH",
        dt::FormatKind::Date,
        1704067200,
        dt::CalendarSystem::Buddhist,
        dt::TimeZone::UTC,
        0,
    )) {
        Ok(r) => {
            let d = match r.calendar_date {
                Some(d) => d,
                None => return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail"),
            };
            if d.year != 2024 + 543 || d.era != "BE" {
                return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C06-历法-近似必须显式", false, "self-check-fail"),
    }
    Verdict::new("C06-历法-近似必须显式", true, "")
}

// ---------------------------------------------------------------------------
// 判据四：时区断言
// ---------------------------------------------------------------------------

fn tz_asserted() -> Verdict {
    // 非法时区一律断言拒绝（非整分钟 / UTC 声称有夏令时）。
    let (non_minute, _) = dt::TimeZone::from_offset(3600 + 30, false);
    match non_minute.assert_valid() {
        Ok(_) => return Verdict::new("C06-时区-非法一律断言", false, "self-check-fail"),
        Err(e) => {
            if e.code != dt::E_TZ_ASSERT || !e.is_complete() {
                return Verdict::new("C06-时区-非法一律断言", false, "self-check-fail");
            }
        }
    }
    let utc_dst = dt::TimeZone {
        offset_seconds: 0,
        has_dst: true,
    };
    if utc_dst.assert_valid().is_ok() {
        return Verdict::new("C06-时区-非法一律断言", false, "self-check-fail");
    }
    // 合法时区必须放行（否则等于把所有时区都拦了）。
    if dt::TimeZone::UTC.assert_valid().is_err() {
        return Verdict::new("C06-时区-非法一律断言", false, "self-check-fail");
    }
    let (tz8, _) = dt::TimeZone::from_offset(8 * 3600, false);
    if tz8.assert_valid().is_err() {
        return Verdict::new("C06-时区-非法一律断言", false, "self-check-fail");
    }
    Verdict::new("C06-时区-非法一律断言", true, "")
}

fn tz_clamped_visible() -> Verdict {
    // 偏移钳制必须**可见**（不静默改数）。
    let (over, rec) = dt::TimeZone::from_offset(100 * 3600, false);
    let r = match rec {
        Some(x) => x,
        None => return Verdict::new("C06-时区-偏移钳制可见", false, "self-check-fail"),
    };
    if r.field != "tz-offset" {
        return Verdict::new("C06-时区-偏移钳制可见", false, "self-check-fail");
    }
    if over.offset_seconds != dt::TimeZone::MAX_OFFSET_SECONDS {
        return Verdict::new("C06-时区-偏移钳制可见", false, "self-check-fail");
    }
    // 在界内不留痕（不 spurious 报警）。
    let (_, ok_rec) = dt::TimeZone::from_offset(3600, false);
    if ok_rec.is_some() {
        return Verdict::new("C06-时区-偏移钳制可见", false, "self-check-fail");
    }
    Verdict::new("C06-时区-偏移钳制可见", true, "")
}

fn tz_shifts_local_time() -> Verdict {
    // 时区必须**真的**改变本地时刻（否则时区就是装饰）。
    let (tz8, _) = dt::TimeZone::from_offset(8 * 3600, false);
    match dt::format(&dt::FormatRequest::new(
        "zh-CN",
        dt::FormatKind::Time,
        0,
        dt::CalendarSystem::Gregorian,
        tz8,
        0,
    )) {
        Ok(r) => {
            if r.text != "08:00:00" {
                return Verdict::new("C06-时区-本地时刻真的偏移", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C06-时区-本地时刻真的偏移", false, "self-check-fail"),
    }
    let (tzm5, _) = dt::TimeZone::from_offset(-5 * 3600, false);
    match dt::format(&dt::FormatRequest::new(
        "en-US",
        dt::FormatKind::Time,
        0,
        dt::CalendarSystem::Gregorian,
        tzm5,
        0,
    )) {
        Ok(r) => {
            if r.text != "19:00:00" {
                return Verdict::new("C06-时区-本地时刻真的偏移", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C06-时区-本地时刻真的偏移", false, "self-check-fail"),
    }
    Verdict::new("C06-时区-本地时刻真的偏移", true, "")
}

// ---------------------------------------------------------------------------
// 判据五：热表单源
// ---------------------------------------------------------------------------

fn cache_key_complete() -> Verdict {
    // 缓存键必须含历法/时区/类别/小数位。
    // 反例：键里若缺时区，则"同一时刻在东京排一遍、在纽约排一遍"会共用条目，
    // 于是后者拿到前者时刻——这类错极难查。
    let mk = |cal, off, kind, fd| {
        let (tz, _) = dt::TimeZone::from_offset(off, false);
        dt::FormatRequest::new("en-US", kind, 0, cal, tz, fd).cache_key()
    };
    let base = mk(dt::CalendarSystem::Gregorian, 0, dt::FormatKind::Date, 0);
    if base == mk(dt::CalendarSystem::Hebrew, 0, dt::FormatKind::Date, 0) {
        return Verdict::new("C06-缓存-键含历法时区类别小数位", false, "self-check-fail");
    }
    if base == mk(dt::CalendarSystem::Gregorian, 3600, dt::FormatKind::Date, 0) {
        return Verdict::new("C06-缓存-键含历法时区类别小数位", false, "self-check-fail");
    }
    if base == mk(dt::CalendarSystem::Gregorian, 0, dt::FormatKind::Time, 0) {
        return Verdict::new("C06-缓存-键含历法时区类别小数位", false, "self-check-fail");
    }
    if base == mk(dt::CalendarSystem::Gregorian, 0, dt::FormatKind::Date, 3) {
        return Verdict::new("C06-缓存-键含历法时区类别小数位", false, "self-check-fail");
    }
    // 同输入必得同键（缓存的前提）。
    if base != mk(dt::CalendarSystem::Gregorian, 0, dt::FormatKind::Date, 0) {
        return Verdict::new("C06-缓存-键含历法时区类别小数位", false, "self-check-fail");
    }
    Verdict::new("C06-缓存-键含历法时区类别小数位", true, "")
}

fn cache_key_has_cldr_version() -> Verdict {
    // 键必须含 CLDR 版本锚——规则集换版后旧缓存不能复用。
    let k = dt::FormatRequest::new(
        "en-US",
        dt::FormatKind::Date,
        0,
        dt::CalendarSystem::Gregorian,
        dt::TimeZone::UTC,
        0,
    )
    .cache_key();
    if !k.contains(dt::CLDR_VERSION_TAG) {
        return Verdict::new("C06-缓存-键含cldr版本", false, "self-check-fail");
    }
    // 键长不得越界（超界会被钳，钳了就说明 Locale 标签异常长）。
    if k.len() > dt::MAX_CACHE_KEY_LEN * 2 {
        return Verdict::new("C06-缓存-键含cldr版本", false, "self-check-fail");
    }
    Verdict::new("C06-缓存-键含cldr版本", true, "")
}

fn hot_table_single_source() -> Verdict {
    // 热表 owner 必须是 F3242，且本项只是 consumer。
    let c = match dt::DATETIME_SINGLE_SOURCE.iter().find(|c| c.key == "hot-table-pool") {
        Some(x) => x,
        None => return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail"),
    };
    if c.owner != "VE-F3242" {
        return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail");
    }
    if !c.consumers.contains(&"VE-F4006") {
        return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail");
    }
    // 本项**不得**自称热表 owner（那会越界代做 F3242）。
    if dt::DATETIME_SINGLE_SOURCE
        .iter()
        .any(|x| x.key != "hot-table-pool" && x.owner == "VE-F4006" && x.label.contains("热表"))
    {
        return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail");
    }
    // 时区owner 必须是 F3694。
    match dt::DATETIME_SINGLE_SOURCE.iter().find(|c| c.key == "host-timezone") {
        Some(x) => {
            if x.owner != "VE-F3694" {
                return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail");
            }
        }
        None => return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail"),
    }
    // 单源唯一性机检必须绿。
    if dt::check_single_source().is_err() {
        return Verdict::new("C06-单源-热表owner唯一", false, "self-check-fail");
    }
    Verdict::new("C06-单源-热表owner唯一", true, "")
}

fn reserved_not_lied() -> Verdict {
    // 预留槽位全未落地且有谎报机检；两条复用槽位必须齐全。
    if dt::check_reserved().is_err() {
        return Verdict::new("C06-预留-未落地不谎报", false, "self-check-fail");
    }
    if dt::RESERVED_SLOTS.iter().any(|s| s.landed) {
        return Verdict::new("C06-预留-未落地不谎报", false, "self-check-fail");
    }
    for up in ["VE-F3694", "VE-F3242"] {
        if !dt::RESERVED_SLOTS.iter().any(|s| s.upstream == up) {
            return Verdict::new("C06-预留-未落地不谎报", false, "self-check-fail");
        }
    }
    Verdict::new("C06-预留-未落地不谎报", true, "")
}

fn spec_numbering_continuous() -> Verdict {
    // 规格表编号从 1 起连续，且每条有标签与强制它的自检项名。
    for (i, item) in dt::SPEC_SHEET.iter().enumerate() {
        if item.no != (i + 1) as u16 {
            return Verdict::new("C06-预留-规格编号连续", false, "self-check-fail");
        }
        if item.label.is_empty() || item.enforced_by.is_empty() || item.key.is_empty() {
            return Verdict::new("C06-预留-规格编号连续", false, "self-check-fail");
        }
    }
    Verdict::new("C06-预留-规格编号连续", true, "")
}

fn criteria_covered() -> Verdict {
    // 判据与规格表双向对齐；锚点五条判据逐条在册。
    if dt::check_spec_coverage().is_err() {
        return Verdict::new("C06-预留-判据全覆盖", false, "self-check-fail");
    }
    for crit in ["CLDR锚定", "五类格式", "多历法", "时区断言", "热表单源"] {
        if !dt::CRITERIA.iter().any(|(c, _)| *c == crit) {
            return Verdict::new("C06-预留-判据全覆盖", false, "self-check-fail");
        }
    }
    if dt::check_zero_privacy().is_err() {
        return Verdict::new("C06-预留-判据全覆盖", false, "self-check-fail");
    }
    Verdict::new("C06-预留-判据全覆盖", true, "")
}

// ---------------------------------------------------------------------------
// 反假门禁
// ---------------------------------------------------------------------------

fn assertions_can_fail() -> Verdict {
    // 用故意错误的输入撞守卫，证明守卫非恒真。
    // 1) 域外历法必须被拒。
    if dt::CalendarSystem::guard("julian").is_ok() {
        return Verdict::new("C06-反假-断言能失败", false, "self-check-fail");
    }
    // 2) 非法时区必须阻断格式化。
    let bad_tz = dt::TimeZone {
        offset_seconds: 90,
        has_dst: false,
    };
    if dt::format(&dt::FormatRequest::new(
        "en-US",
        dt::FormatKind::Date,
        0,
        dt::CalendarSystem::Gregorian,
        bad_tz,
        0,
    ))
    .is_ok()
    {
        return Verdict::new("C06-反假-断言能失败", false, "self-check-fail");
    }
    // 3) 纪元越界必须拒。
    if dt::civil_from_epoch(dt::MAX_EPOCH_SEC + 1).is_ok() {
        return Verdict::new("C06-反假-断言能失败", false, "self-check-fail");
    }
    // 4) 域外格式类别必须被拒。
    if dt::FormatKind::guard("datetime").is_ok() {
        return Verdict::new("C06-反假-断言能失败", false, "self-check-fail");
    }
    Verdict::new("C06-反假-断言能失败", true, "")
}

fn non_ascii_no_crash() -> Verdict {
    // 非 ASCII 与畸形输入不得 panic（内核铁律：异常零静默不靠 panic 实现）。
    for tag in ["汉", "*", "-CN", "x-private", "", "zh-Hans-CN", "İstanbul"] {
        let _ = dt::lookup_rule(tag);
        let _ = dt::primary_subtag(tag);
        if dt::format(&dt::FormatRequest::new(
            tag,
            dt::FormatKind::Date,
            0,
            dt::CalendarSystem::Gregorian,
            dt::TimeZone::UTC,
            0,
        ))
        .is_err()
        {
            return Verdict::new("C06-反假-非ASCII不崩", false, "self-check-fail");
        }
    }
    // 非 ASCII 历法名与类别名同样只该被拒，不该崩。
    for raw in ["汉", "İslamic", "ß", "🙂"] {
        let _ = dt::CalendarSystem::guard(raw);
        let _ = dt::FormatKind::guard(raw);
    }
    Verdict::new("C06-反假-非ASCII不崩", true, "")
}

fn five_kinds_truly_differ() -> Verdict {
    // **反假门禁·专打"五类只是标签"**：若五类格式化实现退化成同一个函数，
    // 这项会立刻变红。
    //
    // 判据：把同一 Locale、同一纪元送进五类，取产物两两比较——
    // 只要有任意一对相同即判红。附加：时间必含冒号、货币必带符号、
    // 百分比必带百分号、日期必不含冒号。
    let base = 1704067200i64;
    let mut got: Vec<String> = Vec::new();
    for k in dt::FormatKind::ALL.iter() {
        match dt::format(&dt::FormatRequest::new(
            "en-US",
            *k,
            base,
            dt::CalendarSystem::Gregorian,
            dt::TimeZone::UTC,
            2,
        )) {
            Ok(r) => got.push(r.text),
            Err(_) => return Verdict::new("C06-反假-五类真不同", false, "self-check-fail"),
        }
    }
    if got.len() != 5 {
        return Verdict::new("C06-反假-五类真不同", false, "self-check-fail");
    }
    for i in 0..got.len() {
        for j in (i + 1)..got.len() {
            if got[i] == got[j] {
                return Verdict::new("C06-反假-五类真不同", false, "self-check-fail");
            }
        }
    }
    // 形态特征反查：冒号只在时间出现。
    if !got[1].contains(':') {
        return Verdict::new("C06-反假-五类真不同", false, "self-check-fail");
    }
    for (idx, k) in dt::FormatKind::ALL.iter().enumerate() {
        if *k != dt::FormatKind::Time && got[idx].contains(':') {
            return Verdict::new("C06-反假-五类真不同", false, "self-check-fail");
        }
    }
    Verdict::new("C06-反假-五类真不同", true, "")
}

/// 替述样例（无障碍：文档替述可读）。过程产物，不参与门禁。
#[allow(dead_code)]
pub fn describe_for_screen_reader(locale: &str, epoch: i64) -> String {
    match dt::format(&dt::FormatRequest::new(
        locale,
        dt::FormatKind::Date,
        epoch,
        dt::CalendarSystem::Gregorian,
        dt::TimeZone::UTC,
        0,
    )) {
        Ok(r) => format!(
            "日期 {}；Locale {}（{}）{}",
            r.text,
            r.effective_locale,
            r.rule_match,
            if r.degraded {
                "；该语言格式未收录，已用通用格式"
            } else {
                ""
            }
        ),
        Err(e) => format!("无法格式化：{}", e.describe()),
    }
}
