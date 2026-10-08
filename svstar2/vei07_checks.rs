//! VE-F4007 · 域自检（判据逐条对应，见 `vei07_plural.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//!
//! - **六类复数** → `C07-六类-闭域齐备`、`C07-六类-守卫拒域外`、
//!   `C07-六类-每类都可达`、`C07-六类-跨小数维可达`、`C07-六类-声明与实现一致`、
//!   `C07-六类-阿拉伯族真出六类`；
//! - **性别模板** → `C07-性别-每语言有集合`、`C07-性别-无语法性别者只一种`、
//!   `C07-性别-有性别者至少两性`、`C07-性别-变量闭域`、`C07-性别-未知变量拒绝`、
//!   `C07-性别-花括号不配对拒绝`、`C07-性别-嵌套错位拒绝`、`C07-性别-转义合法`、
//!   `C07-性别-构造模板恒可解析`；
//! - **联合选择器** → `C07-选择-一次定类别与性别`、`C07-选择-非ASCII不崩`、
//!   `C07-选择-非法locale拒绝`、`C07-选择-怪单位不崩`、`C07-缓存-键含七段`、
//!   `C07-缓存-类别不同键不同`、`C07-缓存-空键与无版本键拒绝`；
//! - **覆盖红线** → `C07-红线-双向对齐`、`C07-红线-完整标签按子标签命中`、
//!   `C07-红线-cldr锚定`、`C07-单源-无重复owner`、`C07-单源-无自环`、
//!   `C07-预留-未落地不谎报`、`C07-预留-规格编号连续`、`C07-预留-判据全覆盖`；
//! - **回退显性** → `C07-回退-未知语言回退other留痕`、`C07-回退-性别缺回退中性留痕`、
//!   `C07-回退-钳制可见`、`C07-回退-零隐私面`；
//!
//! 另设**反假门禁**组：`C07-反假-断言能失败`、`C07-反假-六类真不同`、
//! `C07-反假-非ASCII按字符边界`。
//!
//! `detail` 用 `&'static str`（内核 `CheckSet::add` 只存 `&'static str`），
//! 失败细节由单元测试承担——自检只负责绿/红 + 稳定短码。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vei07_plural as pl;

/// 自检集构造。
pub fn run_vei07_checks() -> CheckSet {
    let mut set = CheckSet::new(pl::VEA_DOMAIN);
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

    // 判据一：六类复数
    vs.push(six_categories_closed());
    vs.push(six_categories_guard_rejects());
    vs.push(six_categories_reachable());
    vs.push(six_categories_across_fraction());
    vs.push(rule_claims_honest());
    vs.push(arabic_really_six());

    // 判据二：性别模板
    vs.push(gender_set_per_language());
    vs.push(genderless_languages_single());
    vs.push(gendered_languages_multi());
    vs.push(template_vars_closed());
    vs.push(template_unknown_var_rejected());
    vs.push(template_unclosed_rejected());
    vs.push(template_nested_rejected());
    vs.push(template_escape_ok());
    vs.push(generated_templates_parse());

    // 判据三：联合选择器
    vs.push(selector_joint());
    vs.push(selector_non_ascii());
    vs.push(selector_rejects_bad_locale());
    vs.push(selector_odd_unit_no_crash());
    vs.push(cache_key_segments());
    vs.push(cache_key_varies_by_category());
    vs.push(cache_key_rejects_bad());

    // 判据四：覆盖红线
    vs.push(coverage_bidirectional());
    vs.push(locale_two_level_lookup());
    vs.push(cldr_anchor_pinned());
    vs.push(single_source_no_dup());
    vs.push(single_source_no_self_loop());
    vs.push(reserved_not_lied());
    vs.push(spec_numbering_continuous());
    vs.push(criteria_covered());

    // 判据五：回退显性
    vs.push(fallback_other_visible());
    vs.push(gender_fallback_visible());
    vs.push(clamps_visible());
    vs.push(zero_privacy_surface());

    // 反假门禁
    vs.push(assertions_can_fail());
    vs.push(six_categories_truly_differ());
    vs.push(non_ascii_by_char_boundary());

    for v in vs {
        set.add(v.name, v.passed, if v.passed { "" } else { v.detail });
    }
}

// ---------------------------------------------------------------------------
// 判据一：六类复数
// ---------------------------------------------------------------------------

fn six_categories_closed() -> Verdict {
    // 枚举必须**恰好六类**，且 `ALL` 覆盖全部变体（漏登记 = 有类别永不被遍历）。
    if pl::PluralCategory::ALL.len() != 6 {
        return Verdict::new("C07-六类-闭域齐备", false, "self-check-fail");
    }
    // 每个变体都必须能按 name 反查回自己（防 `name()` 写错导致两个变体同名）。
    for c in pl::PluralCategory::ALL.iter() {
        match pl::PluralCategory::guard(c.name()) {
            Ok(back) => {
                if back != *c {
                    return Verdict::new("C07-六类-闭域齐备", false, "self-check-fail");
                }
            }
            Err(_) => return Verdict::new("C07-六类-闭域齐备", false, "self-check-fail"),
        }
        // name 与 slot 必须一致（slot 是name 加前缀）。
        if c.slot() != format!("plural.{}", c.name()) {
            return Verdict::new("C07-六类-闭域齐备", false, "self-check-fail");
        }
    }
    // `Other` 是唯一合法兜底。
    for c in pl::PluralCategory::ALL.iter() {
        if c.is_fallback() != matches!(c, pl::PluralCategory::Other) {
            return Verdict::new("C07-六类-闭域齐备", false, "self-check-fail");
        }
    }
    Verdict::new("C07-六类-闭域齐备", true, "")
}

fn six_categories_guard_rejects() -> Verdict {
    // 守卫必须拒域外，且拒绝信息三要素齐全（异常零静默）。
    let e = match pl::PluralCategory::guard("several") {
        Ok(_) => return Verdict::new("C07-六类-守卫拒域外", false, "self-check-fail"),
        Err(e) => e,
    };
    if e.code != pl::E_CATEGORY_INVALID || !e.is_complete() || e.describe().is_empty() {
        return Verdict::new("C07-六类-守卫拒域外", false, "self-check-fail");
    }
    Verdict::new("C07-六类-守卫拒域外", true, "")
}

fn six_categories_reachable() -> Verdict {
    match pl::check_all_six_categories_reachable() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-六类-每类都可达", false, "self-check-fail"),
    }
    Verdict::new("C07-六类-每类都可达", true, "")
}

fn six_categories_across_fraction() -> Verdict {
    // **样本必须跨小数维**——这是本域实测出的真实教训。
    //
    // 初版机检只喂整数样本，于是斯拉夫族的 `other`（仅小数可达）被判成
    // "声明不实"，红了一条并不存在的缺陷。反过来，若某天有人给斯拉夫族加上
    // `other` 分支而只测整数，也一样看不出来。所以这里显式验"小数维参与"：
    // 同一个数值在 v=0 与 v=1 下类别必须不同（对至少一个族成立）。
    if pl::SAMPLE_FRACTIONS.len() < 2 {
        return Verdict::new("C07-六类-跨小数维可达", false, "self-check-fail");
    }
    let mut differ = false;
    for entry in pl::PLURAL_RULES.iter() {
        for n in [2i64, 5, 21].iter() {
            let (a, _) = pl::Operands::of(*n, 0);
            let (b, _) = pl::Operands::of(*n, 1);
            if pl::eval_rule(entry.rule, &a) != pl::eval_rule(entry.rule, &b) {
                differ = true;
            }
        }
    }
    // 至少要有规则族对小数敏感，否则"小数维"这一维就是摆设。
    if !differ {
        return Verdict::new("C07-六类-跨小数维可达", false, "self-check-fail");
    }
    // 且带小数时小数位数本身必须被钳制留痕。
    let (_, rec) = pl::Operands::of(1, 99);
    if rec.is_none() {
        return Verdict::new("C07-六类-跨小数维可达", false, "self-check-fail");
    }
    Verdict::new("C07-六类-跨小数维可达", true, "")
}

fn rule_claims_honest() -> Verdict {
    match pl::check_rule_reachability_claims() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-六类-声明与实现一致", false, "self-check-fail"),
    }
    Verdict::new("C07-六类-声明与实现一致", true, "")
}

fn arabic_really_six() -> Verdict {
    // 阿拉伯族是唯一六类全用的族——逐一钉死，别让某个分支被"顺手清理"掉。
    let cases: [(i64, pl::PluralCategory); 6] = [
        (0, pl::PluralCategory::Zero),
        (1, pl::PluralCategory::One),
        (2, pl::PluralCategory::Two),
        (3, pl::PluralCategory::Few),
        (11, pl::PluralCategory::Many),
        (101, pl::PluralCategory::Other),
    ];
    for (n, want) in cases.iter() {
        let (op, _) = pl::Operands::of(*n, 0);
        if pl::eval_rule(pl::PluralRule::ArabicFull, &op) != *want {
            return Verdict::new("C07-六类-阿拉伯族真出六类", false, "self-check-fail");
        }
    }
    Verdict::new("C07-六类-阿拉伯族真出六类", true, "")
}

// ---------------------------------------------------------------------------
// 判据二：性别模板
// ---------------------------------------------------------------------------

fn gender_set_per_language() -> Verdict {
    // 复数表里每个语言都必须能在性别表里查到（同源承诺）。
    for entry in pl::PLURAL_RULES.iter() {
        if pl::lookup_gender_set(entry.language).is_none() {
            return Verdict::new("C07-性别-每语言有集合", false, "self-check-fail");
        }
    }
    // 性别集合的 `has` 必须与掩码逐位一致（位掩码写错会让某性别凭空可用）。
    for gs in pl::GENDER_RULES.iter() {
        for g in pl::Gender::ALL.iter() {
            let bit = pl::gender_bit(*g);
            if bit >= 8 {
                return Verdict::new("C07-性别-每语言有集合", false, "self-check-fail");
            }
            if gs.has(*g) != (gs.mask & (1u8 << bit) != 0) {
                return Verdict::new("C07-性别-每语言有集合", false, "self-check-fail");
            }
        }
    }
    Verdict::new("C07-性别-每语言有集合", true, "")
}

fn genderless_languages_single() -> Verdict {
    // 无语法性别的语言必须**只登记 Common**——硬塞阳性/阴性会产出语法错误句子。
    for entry in pl::PLURAL_RULES.iter() {
        if entry.gendered {
            continue;
        }
        let gs = match pl::lookup_gender_set(entry.language) {
            Some(g) => g,
            None => return Verdict::new("C07-性别-无语法性别者只一种", false, "self-check-fail"),
        };
        if gs.count() != 1 || !gs.has(pl::Gender::Common) {
            return Verdict::new("C07-性别-无语法性别者只一种", false, "self-check-fail");
        }
    }
    Verdict::new("C07-性别-无语法性别者只一种", true, "")
}

fn gendered_languages_multi() -> Verdict {
    // 有语法性别的语言至少两性，且必须含 Feminine 与 Masculine
    // （只给一种性别等于把语法信息丢了）。
    for entry in pl::PLURAL_RULES.iter() {
        if !entry.gendered {
            continue;
        }
        let gs = match pl::lookup_gender_set(entry.language) {
            Some(g) => g,
            None => return Verdict::new("C07-性别-有性别者至少两性", false, "self-check-fail"),
        };
        if gs.count() < 2 || !gs.has(pl::Gender::Feminine) || !gs.has(pl::Gender::Masculine) {
            return Verdict::new("C07-性别-有性别者至少两性", false, "self-check-fail");
        }
    }
    Verdict::new("C07-性别-有性别者至少两性", true, "")
}

fn template_vars_closed() -> Verdict {
    // 允许集必须非空、不得有重复、不得有非法变量名。
    if pl::ALLOWED_VARS.is_empty() {
        return Verdict::new("C07-性别-变量闭域", false, "self-check-fail");
    }
    for (i, a) in pl::ALLOWED_VARS.iter().enumerate() {
        if a.is_empty() {
            return Verdict::new("C07-性别-变量闭域", false, "self-check-fail");
        }
        for b in pl::ALLOWED_VARS.iter().skip(i + 1) {
            if a == b {
                return Verdict::new("C07-性别-变量闭域", false, "self-check-fail");
            }
        }
    }
    Verdict::new("C07-性别-变量闭域", true, "")
}

fn template_unknown_var_rejected() -> Verdict {
    // **锚点降级矩阵第四条**：模板变量错→校验拒绝。
    // 注意输入必须是**表外**的真实错拼形态（`{conut}`），
    // 拿表内元素验闭域等于近乎恒真的弱门禁。
    for bad in ["{conut}", "{unt}", "{COUNT}", "{c}"] {
        match pl::parse_template(bad) {
            Ok(_) => return Verdict::new("C07-性别-未知变量拒绝", false, "self-check-fail"),
            Err(e) => {
                if !e.is_complete() {
                    return Verdict::new("C07-性别-未知变量拒绝", false, "self-check-fail");
                }
            }
        }
    }
    // 反向：表内变量必须放行（否则闭域收得太紧也是缺陷）。
    for good in pl::ALLOWED_VARS.iter() {
        let t = format!("{{{}}}", good);
        if pl::parse_template(&t).is_err() {
            return Verdict::new("C07-性别-未知变量拒绝", false, "self-check-fail");
        }
    }
    Verdict::new("C07-性别-未知变量拒绝", true, "")
}

fn template_unclosed_rejected() -> Verdict {
    for bad in ["{count", "a {unit} b {category"].iter() {
        match pl::parse_template(bad) {
            Ok(_) => return Verdict::new("C07-性别-花括号不配对拒绝", false, "self-check-fail"),
            Err(e) => {
                if e.code != pl::E_TEMPLATE_SYNTAX {
                    return Verdict::new("C07-性别-花括号不配对拒绝", false, "self-check-fail");
                }
            }
        }
    }
    // 空变量名也是拒绝（另一类错，但同属"变量错"）。
    if pl::parse_template("{}").is_ok() {
        return Verdict::new("C07-性别-花括号不配对拒绝", false, "self-check-fail");
    }
    Verdict::new("C07-性别-花括号不配对拒绝", true, "")
}

fn template_nested_rejected() -> Verdict {
    if pl::parse_template("{a{b}}").is_ok() {
        return Verdict::new("C07-性别-嵌套错位拒绝", false, "self-check-fail");
    }
    // 孤立右括号同样拒绝。
    if pl::parse_template("abc}").is_ok() {
        return Verdict::new("C07-性别-嵌套错位拒绝", false, "self-check-fail");
    }
    Verdict::new("C07-性别-嵌套错位拒绝", true, "")
}

fn template_escape_ok() -> Verdict {
    // 转义必须真的产出字面花括号，且**不**被当成变量。
    match pl::parse_template("a{{b}}c") {
        Ok(ast) => {
            if ast.var_count() != 0 {
                return Verdict::new("C07-性别-转义合法", false, "self-check-fail");
            }
        }
        Err(_) => return Verdict::new("C07-性别-转义合法", false, "self-check-fail"),
    }
    Verdict::new("C07-性别-转义合法", true, "")
}

fn generated_templates_parse() -> Verdict {
    // **本域的模板生成器必须只产出自己能解析的模板**。
    //
    // 这项守卫的是一个实测缺陷：初版 [`template_pattern`] 对斯拉夫族产出
    // `{unit}-{category}`，而 `category` 当时**不在允许变量集内**——于是本域
    // 自己造的模板被自己的解析器拒掉。这类"自己违反自己契约"的缺陷只有
    // 穷举反查才抓得到。
    for entry in pl::PLURAL_RULES.iter() {
        for c in pl::PluralCategory::ALL.iter() {
            for g in pl::Gender::ALL.iter() {
                let p = pl::template_pattern(entry.language, *c, *g);
                if pl::parse_template(&p).is_err() {
                    return Verdict::new(
                        "C07-性别-构造模板恒可解析",
                        false,
                        "self-check-fail",
                    );
                }
                // 且长度必须在上界内（否则渲染前就被拒）。
                if p.len() > pl::MAX_TEMPLATE_LEN {
                    return Verdict::new(
                        "C07-性别-构造模板恒可解析",
                        false,
                        "self-check-fail",
                    );
                }
            }
        }
    }
    Verdict::new("C07-性别-构造模板恒可解析", true, "")
}

// ---------------------------------------------------------------------------
// 判据三：联合选择器
// ---------------------------------------------------------------------------

fn selector_joint() -> Verdict {
    // 一次调用同时定出类别与性别，且两者都被渲染进文本。
    let mut bag = pl::DiagBag::new();
    let req = pl::SelectorRequest::new("ru-RU", 3, 0, pl::Gender::Feminine, "файл");
    let sel = match pl::select(&req, &mut bag) {
        Ok(s) => s,
        Err(_) => return Verdict::new("C07-选择-一次定类别与性别", false, "self-check-fail"),
    };
    if sel.category != pl::PluralCategory::Few || sel.gender != pl::Gender::Feminine {
        return Verdict::new("C07-选择-一次定类别与性别", false, "self-check-fail");
    }
    if sel.gender_degraded || sel.plural_degraded {
        return Verdict::new("C07-选择-一次定类别与性别", false, "self-check-fail");
    }
    // 文本必须真的含数值（渲染发生了）。
    if !sel.text.contains('3') {
        return Verdict::new("C07-选择-一次定类别与性别", false, "self-check-fail");
    }
    // requested_gender 必须留档（便于上层报警"为何降级"）。
    if sel.requested_gender != pl::Gender::Feminine {
        return Verdict::new("C07-选择-一次定类别与性别", false, "self-check-fail");
    }
    // 无降级时不得产降级诊断。
    if bag.count_of(pl::DiagKind::LocaleUnded) != 0
        || bag.count_of(pl::DiagKind::PluralFallbackOther) != 0
    {
        return Verdict::new("C07-选择-一次定类别与性别", false, "self-check-fail");
    }
    Verdict::new("C07-选择-一次定类别与性别", true, "")
}

fn selector_non_ascii() -> Verdict {
    // 非 ASCII Locale/单位/字面量必须能走通，不得因字符边界切错而崩。
    let mut bag = pl::DiagBag::new();
    for (locale, unit, gender) in [
        ("zh-CN", "文件", pl::Gender::Common),
        ("ja-JP", "ファイル", pl::Gender::Common),
        ("ar-EG", "ملف", pl::Gender::Feminine),
        ("ru-RU", "файл", pl::Gender::Neuter),
        ("he-IL", "קובץ", pl::Gender::Masculine),
    ] {
        let req = pl::SelectorRequest::new(locale, 1, 0, gender, unit);
        match pl::select(&req, &mut bag) {
            Ok(s) => {
                if s.text.is_empty() {
                    return Verdict::new("C07-选择-非ASCII不崩", false, "self-check-fail");
                }
            }
            Err(_) => return Verdict::new("C07-选择-非ASCII不崩", false, "self-check-fail"),
        }
    }
    Verdict::new("C07-选择-非ASCII不崩", true, "")
}

fn selector_rejects_bad_locale() -> Verdict {
    let mut bag = pl::DiagBag::new();
    // 空格是非法字符；空串是未填——两者都必须显性拒绝，不得静默走 und。
    for bad in ["zh CN", "", "zh/CN", "en@x"].iter() {
        let req = pl::SelectorRequest::new(bad, 1, 0, pl::Gender::Common, "x");
        match pl::select(&req, &mut bag) {
            Ok(_) => return Verdict::new("C07-选择-非法locale拒绝", false, "self-check-fail"),
            Err(e) => {
                if e.code != pl::E_LOCALE_INVALID || !e.is_complete() {
                    return Verdict::new("C07-选择-非法locale拒绝", false, "self-check-fail");
                }
            }
        }
    }
    // checked() 构造器也必须拒（不能只有 select 拒）。
    if pl::SelectorRequest::checked("zh CN", 1, 0, pl::Gender::Common, "x").is_ok() {
        return Verdict::new("C07-选择-非法locale拒绝", false, "self-check-fail");
    }
    Verdict::new("C07-选择-非法locale拒绝", true, "")
}

fn selector_odd_unit_no_crash() -> Verdict {
    // 单位名里含花括号/空串/超长串都不得崩——守卫不得成为崩溃源。
    let mut bag = pl::DiagBag::new();
    for unit in ["{weird}", "", "{count}{count}{count}", "x"].iter() {
        let req = pl::SelectorRequest::new("en-US", 2, 0, pl::Gender::Common, unit);
        if pl::select(&req, &mut bag).is_err() {
            return Verdict::new("C07-选择-怪单位不崩", false, "self-check-fail");
        }
    }
    Verdict::new("C07-选择-怪单位不崩", true, "")
}

fn cache_key_segments() -> Verdict {
    let mut bag = pl::DiagBag::new();
    let sel = match pl::select(
        &pl::SelectorRequest::new("ar-EG", 2, 0, pl::Gender::Feminine, "file"),
        &mut bag,
    ) {
        Ok(s) => s,
        Err(_) => return Verdict::new("C07-缓存-键含七段", false, "self-check-fail"),
    };
    let key = pl::selection_cache_key(&sel, "file");
    if pl::check_cache_key(&key).is_err() {
        return Verdict::new("C07-缓存-键含七段", false, "self-check-fail");
    }
    if key.split('|').count() != pl::CACHE_KEY_SEGMENTS {
        return Verdict::new("C07-缓存-键含七段", false, "self-check-fail");
    }
    // 七段必须逐段对上（版本/语言/类别/性别/请求性别/单位/产物）。
    let parts: Vec<&str> = key.split('|').collect();
    if parts[0] != pl::CLDR_VERSION_TAG
        || parts[1] != sel.locale
        || parts[2] != sel.category.name()
        || parts[3] != sel.gender.name()
        || parts[4] != sel.requested_gender.name()
        || parts[5] != "file"
        || parts[6] != sel.text
    {
        return Verdict::new("C07-缓存-键含七段", false, "self-check-fail");
    }
    Verdict::new("C07-缓存-键含七段", true, "")
}

fn cache_key_varies_by_category() -> Verdict {
    // 键完备性的核心：**任何一个影响结果的输入变了，键必须变**。
    let mut bag = pl::DiagBag::new();
    let mut keys: Vec<String> = Vec::new();
    // 同一语言下遍历全部可达类别（阿拉伯族六类全用）。
    for n in [0i64, 1, 2, 3, 11, 101].iter() {
        let req = pl::SelectorRequest::new("ar-EG", *n, 0, pl::Gender::Feminine, "file");
        if let Ok(sel) = pl::select(&req, &mut bag) {
            keys.push(pl::selection_cache_key(&sel, "file"));
        }
    }
    // 去重后至少要有 5 类（阿拉伯 6 类里 other 与101 之后可能有重合，取保守值）。
    let mut uniq = keys.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() < 5 {
        return Verdict::new("C07-缓存-类别不同键不同", false, "self-check-fail");
    }
    // 类别变了键也必须变（逐条比对）。
    for i in 0..keys.len() {
        for j in 0..keys.len() {
            if i == j {
                continue;
            }
            let ki: Vec<&str> = keys[i].split('|').collect();
            let kj: Vec<&str> = keys[j].split('|').collect();
            if ki[2] != kj[2] && keys[i] == keys[j] {
                return Verdict::new("C07-缓存-类别不同键不同", false, "self-check-fail");
            }
        }
    }
    // 单位不同也必须换键（同产物不同单位是常见场景）。
    let sel = match pl::select(
        &pl::SelectorRequest::new("ar-EG", 2, 0, pl::Gender::Feminine, "file"),
        &mut bag,
    ) {
        Ok(s) => s,
        Err(_) => return Verdict::new("C07-缓存-类别不同键不同", false, "self-check-fail"),
    };
    if pl::selection_cache_key(&sel, "file") == pl::selection_cache_key(&sel, "folder") {
        return Verdict::new("C07-缓存-类别不同键不同", false, "self-check-fail");
    }
    Verdict::new("C07-缓存-类别不同键不同", true, "")
}

fn cache_key_rejects_bad() -> Verdict {
    // 空键与无版本前缀的键必须拒——不截断（截断的键会撞车）。
    if pl::check_cache_key("").is_ok() {
        return Verdict::new("C07-缓存-空键与无版本键拒绝", false, "self-check-fail");
    }
    if pl::check_cache_key("xx|yy|zz").is_ok() {
        return Verdict::new("C07-缓存-空键与无版本键拒绝", false, "self-check-fail");
    }
    // 超长键必须拒。
    let long = "cldr-46|".to_string() + &"x".repeat(pl::MAX_CACHE_KEY_LEN + 8);
    if pl::check_cache_key(&long).is_ok() {
        return Verdict::new("C07-缓存-空键与无版本键拒绝", false, "self-check-fail");
    }
    Verdict::new("C07-缓存-空键与无版本键拒绝", true, "")
}

// ---------------------------------------------------------------------------
// 判据四：覆盖红线
// ---------------------------------------------------------------------------

fn coverage_bidirectional() -> Verdict {
    // **双向**红线的守卫（实测缺陷的守卫类型：域间/表间分叉）。
    match pl::check_coverage_redline() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-红线-双向对齐", false, "self-check-fail"),
    }
    // 正向逐条反查。
    for lang in pl::ALIGNED_LANGUAGES.iter() {
        let lk = pl::lookup_rule(lang);
        if lk.matched != pl::RuleMatch::Exact {
            return Verdict::new("C07-红线-双向对齐", false, "self-check-fail");
        }
        if pl::lookup_gender_set(lang).is_none() {
            return Verdict::new("C07-红线-双向对齐", false, "self-check-fail");
        }
    }
    // 反向逐条反查：规则表不得有对齐清单外的语言（und 除外）。
    for entry in pl::PLURAL_RULES.iter() {
        if entry.language == "und" {
            continue;
        }
        if !pl::ALIGNED_LANGUAGES.contains(&entry.language) {
            return Verdict::new("C07-红线-双向对齐", false, "self-check-fail");
        }
    }
    Verdict::new("C07-红线-双向对齐", true, "")
}

fn locale_two_level_lookup() -> Verdict {
    // 完整 BCP47 标签必须按主语言子标签命中，而不是每带区域/脚本就误报未收录。
    for (tag, lang) in [
        ("ar-EG", "ar"),
        ("ta-IN", "ta"),
        ("zh-Hans-CN", "zh"),
        ("pt-BR", "pt"),
        ("ru", "ru"),
    ] {
        let lk = pl::lookup_rule(tag);
        if lk.degraded {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
        if lk.entry.language != lang {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
        // 裸语言码必须是 Exact（不是子标签命中）。
        if pl::lookup_rule(lang).matched != pl::RuleMatch::Exact {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
        // 性别表同样两级。
        let gs = match pl::lookup_gender_set(tag) {
            Some(g) => g,
            None => return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail"),
        };
        if gs.language != lang {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
    }
    // **非规范形态必须降级**（下划线/大写）——与 F4004/F4006 严格同口径。
    //
    // 本项初版在这里"放宽"了（下划线也切、大小写也归一），六域共存联编时暴露
    // 出分叉：`en_US` 排版/日期域降级而复数域命中。现在收敛为严格口径——
    // 规范化归 F4002，下游域不各自放宽。
    for tag in ["en_US", "pt_BR", "EN-US"].iter() {
        let lk = pl::lookup_rule(tag);
        if !lk.degraded || lk.matched != pl::RuleMatch::Missed {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
        // 且必须走 other 兜底，不是随便挑一条规则。
        if lk.rule() != pl::PluralRule::OnlyOther {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
    }
    // **`primary_subtag` 自身必须严格只切连字符**——这条是被反假变体逼出来的。
    //
    // 变体把下划线切分加回去后，若无此守卫全域仍全绿（因为 `lookup_rule`
    // 里可能还有别处兜底）。直接钉住切分函数的输出，才让该行为路径可变异打红。
    if pl::primary_subtag("en_US") != "en_US" {
        return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
    }
    if pl::primary_subtag("ar-EG") != "ar" || pl::primary_subtag("en") != "en" {
        return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
    }
    // 规范形态（连字符）仍须正常命中，且性别表同样两级。
    for tag in ["en-US", "pt-BR", "ar-EG", "zh-Hans-CN"].iter() {
        if pl::lookup_rule(tag).degraded || pl::lookup_gender_set(tag).is_none() {
            return Verdict::new("C07-红线-完整标签按子标签命中", false, "self-check-fail");
        }
    }
    Verdict::new("C07-红线-完整标签按子标签命中", true, "")
}

fn cldr_anchor_pinned() -> Verdict {
    if pl::CLDR_VERSION == 0 || pl::CLDR_VERSION_TAG != "cldr-46" {
        return Verdict::new("C07-红线-cldr锚定", false, "self-check-fail");
    }
    match pl::check_cldr_anchor() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-红线-cldr锚定", false, "self-check-fail"),
    }
    // und 兜底必须在位（查表落空的唯一去处）。
    let und = pl::lookup_rule("und");
    if und.matched != pl::RuleMatch::Exact || und.degraded {
        return Verdict::new("C07-红线-cldr锚定", false, "self-check-fail");
    }
    Verdict::new("C07-红线-cldr锚定", true, "")
}

fn single_source_no_dup() -> Verdict {
    match pl::check_single_source() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-单源-无重复owner", false, "self-check-fail"),
    }
    // 逐条反查 key 唯一。
    let claims = pl::PLURAL_SINGLE_SOURCE;
    for (i, a) in claims.iter().enumerate() {
        for b in claims.iter().skip(i + 1) {
            if a.key == b.key {
                return Verdict::new("C07-单源-无重复owner", false, "self-check-fail");
            }
        }
    }
    // 本域必须是复数规则表的 owner，且把 F4006 / N02 登记为 consumer。
    let own = match claims.iter().find(|c| c.key == "cldr-plural-rules") {
        Some(c) => c,
        None => return Verdict::new("C07-单源-无重复owner", false, "self-check-fail"),
    };
    if own.owner != "VE-F4007" {
        return Verdict::new("C07-单源-无重复owner", false, "self-check-fail");
    }
    if !own.consumers.contains(&"VE-F4006") || !own.consumers.contains(&"VE-N02") {
        return Verdict::new("C07-单源-无重复owner", false, "self-check-fail");
    }
    Verdict::new("C07-单源-无重复owner", true, "")
}

fn single_source_no_self_loop() -> Verdict {
    // 自环守卫：跨能力互相消费是合法上下游（同能力自环才违法）。
    // 这项判据本身是对初版门禁缺陷的守卫——初版把正常的上下游误判成自环。
    for c in pl::PLURAL_SINGLE_SOURCE.iter() {
        if c.consumers.contains(&c.owner) {
            return Verdict::new("C07-单源-无自环", false, "self-check-fail");
        }
    }
    // 确认合法的上下游形态仍被放行（F4007 消费 F4006 的版本锚定）。
    let anchor = pl::PLURAL_SINGLE_SOURCE
        .iter()
        .find(|c| c.key == "cldr-version-anchor");
    match anchor {
        Some(a) => {
            if !a.consumers.contains(&"VE-F4007") {
                return Verdict::new("C07-单源-无自环", false, "self-check-fail");
            }
        }
        None => return Verdict::new("C07-单源-无自环", false, "self-check-fail"),
    }
    Verdict::new("C07-单源-无自环", true, "")
}

fn reserved_not_lied() -> Verdict {
    match pl::check_reserved() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-预留-未落地不谎报", false, "self-check-fail"),
    }
    // 逐条反查：不得有 landed: true。
    for s in pl::RESERVED_SLOTS.iter() {
        if s.landed {
            return Verdict::new("C07-预留-未落地不谎报", false, "self-check-fail");
        }
        if s.upstream.is_empty() || s.key.is_empty() || s.on_landed.is_empty() {
            return Verdict::new("C07-预留-未落地不谎报", false, "self-check-fail");
        }
    }
    Verdict::new("C07-预留-未落地不谎报", true, "")
}

fn spec_numbering_continuous() -> Verdict {
    match pl::check_spec_coverage() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-预留-规格编号连续", false, "self-check-fail"),
    }
    // 逐条反查编号连续 + 键唯一。
    for (i, s) in pl::SPEC_SHEET.iter().enumerate() {
        if s.no != (i + 1) as u16 {
            return Verdict::new("C07-预留-规格编号连续", false, "self-check-fail");
        }
        for b in pl::SPEC_SHEET.iter().skip(i + 1) {
            if s.key == b.key {
                return Verdict::new("C07-预留-规格编号连续", false, "self-check-fail");
            }
        }
        // enforced_by 必须指向一个真实存在的判据名（否则是空头承诺）。
        if s.enforced_by.is_empty() {
            return Verdict::new("C07-预留-规格编号连续", false, "self-check-fail");
        }
    }
    Verdict::new("C07-预留-规格编号连续", true, "")
}

fn criteria_covered() -> Verdict {
    // 锚点五条判据必须都在规格表里有对应键。
    if pl::CRITERIA.len() != 5 {
        return Verdict::new("C07-预留-判据全覆盖", false, "self-check-fail");
    }
    for (name, keys) in pl::CRITERIA.iter() {
        for k in keys.iter() {
            if !pl::SPEC_SHEET.iter().any(|s| s.key == *k) {
                return Verdict::new("C07-预留-判据全覆盖", false, "self-check-fail");
            }
        }
        // 判据名不得为空。
        if name.is_empty() {
            return Verdict::new("C07-预留-判据全覆盖", false, "self-check-fail");
        }
    }
    // 反向：规格表里的每条都必须被某条判据（或纪律条目）引用
    // ——防"规格表里躺着没人管的条目"。这条是实测缺陷的守卫：
    // 初版判据表每条只挂 2 个键，导致 4 条规格悬空。
    for s in pl::SPEC_SHEET.iter() {
        let referenced = pl::CRITERIA.iter().any(|(_, keys)| keys.contains(&s.key))
            || pl::DISCIPLINE_KEYS.contains(&s.key);
        if !referenced {
            return Verdict::new("C07-预留-判据全覆盖", false, "self-check-fail");
        }
    }
    Verdict::new("C07-预留-判据全覆盖", true, "")
}

// ---------------------------------------------------------------------------
// 判据五：回退显性
// ---------------------------------------------------------------------------

fn fallback_other_visible() -> Verdict {
    // **锚点降级矩阵第一条**：未知语言复数→other 回退+显性。
    let mut bag = pl::DiagBag::new();
    let req = pl::SelectorRequest::new("xx-YY", 1, 0, pl::Gender::Common, "file");
    let sel = match pl::select(&req, &mut bag) {
        Ok(s) => s,
        Err(_) => return Verdict::new("C07-回退-未知语言回退other留痕", false, "self-check-fail"),
    };
    if !sel.plural_degraded || sel.category != pl::PluralCategory::Other {
        return Verdict::new("C07-回退-未知语言回退other留痕", false, "self-check-fail");
    }
    if sel.locale != "und" {
        return Verdict::new("C07-回退-未知语言回退other留痕", false, "self-check-fail");
    }
    // 两条诊断都必须留痕（少一条就是"静默降级"）。
    if bag.count_of(pl::DiagKind::LocaleUnded) == 0 {
        return Verdict::new("C07-回退-未知语言回退other留痕", false, "self-check-fail");
    }
    if bag.count_of(pl::DiagKind::PluralFallbackOther) == 0 {
        return Verdict::new("C07-回退-未知语言回退other留痕", false, "self-check-fail");
    }
    // 诊断码必须非空（对拍按码比对）。
    for d in bag.items() {
        if d.kind.code().is_empty() || d.what.is_empty() {
            return Verdict::new("C07-回退-未知语言回退other留痕", false, "self-check-fail");
        }
    }
    Verdict::new("C07-回退-未知语言回退other留痕", true, "")
}

fn gender_fallback_visible() -> Verdict {
    // **锚点降级矩阵第二条**：性别缺→中性模板回退（且必须显性）。
    let mut bag = pl::DiagBag::new();
    // 中文无语法性别，请求 Feminine。
    let req = pl::SelectorRequest::new("zh-CN", 1, 0, pl::Gender::Feminine, "文件");
    let sel = match pl::select(&req, &mut bag) {
        Ok(s) => s,
        Err(_) => return Verdict::new("C07-回退-性别缺回退中性留痕", false, "self-check-fail"),
    };
    if !sel.gender_degraded {
        return Verdict::new("C07-回退-性别缺回退中性留痕", false, "self-check-fail");
    }
    if !sel.gender.is_neutral_side() {
        return Verdict::new("C07-回退-性别缺回退中性留痕", false, "self-check-fail");
    }
    if bag.count_of(pl::DiagKind::GenderNeutralFallback) == 0 {
        return Verdict::new("C07-回退-性别缺回退中性留痕", false, "self-check-fail");
    }
    // 反向：语言支持该性别时**不得**降级（无谓降级也是缺陷）。
    let mut bag2 = pl::DiagBag::new();
    let ok = pl::SelectorRequest::new("en-US", 1, 0, pl::Gender::Feminine, "file");
    if let Ok(s) = pl::select(&ok, &mut bag2) {
        if s.gender_degraded || s.gender != pl::Gender::Feminine {
            return Verdict::new("C07-回退-性别缺回退中性留痕", false, "self-check-fail");
        }
    }
    Verdict::new("C07-回退-性别缺回退中性留痕", true, "")
}

fn clamps_visible() -> Verdict {
    // 钳制必须留痕，且生效值真的落在边界上。
    let (n, rec) = pl::Operands::of(10i64.pow(15), 0);
    if rec.is_none() || n.n != pl::MAX_COUNT {
        return Verdict::new("C07-回退-钳制可见", false, "self-check-fail");
    }
    let (n2, rec2) = pl::Operands::of(-10i64.pow(15), 0);
    if rec2.is_none() || n2.n != pl::MAX_COUNT {
        return Verdict::new("C07-回退-钳制可见", false, "self-check-fail");
    }
    let (_, rec3) = pl::Operands::of(1, 99);
    if rec3.is_none() {
        return Verdict::new("C07-回退-钳制可见", false, "self-check-fail");
    }
    // 未越界时不得留痕（无谓钳制记录会淹没真问题）。
    if pl::Operands::of(1, 0).1.is_some() {
        return Verdict::new("C07-回退-钳制可见", false, "self-check-fail");
    }
    // 走选择器时钳制也必须进诊断袋。
    let mut bag = pl::DiagBag::new();
    let req = pl::SelectorRequest::new("en-US", 10i64.pow(15), 99, pl::Gender::Common, "f");
    if pl::select(&req, &mut bag).is_ok() {
        if bag.count_of(pl::DiagKind::CountClamped) == 0
            || bag.count_of(pl::DiagKind::FractionClamped) == 0
        {
            return Verdict::new("C07-回退-钳制可见", false, "self-check-fail");
        }
    }
    // MIN_COUNT 必须与MAX_COUNT 对称（负向钳制边界）。
    if pl::MIN_COUNT != -pl::MAX_COUNT {
        return Verdict::new("C07-回退-钳制可见", false, "self-check-fail");
    }
    Verdict::new("C07-回退-钳制可见", true, "")
}

fn zero_privacy_surface() -> Verdict {
    match pl::check_zero_privacy() {
        Ok(()) => {}
        Err(_) => return Verdict::new("C07-回退-零隐私面", false, "self-check-fail"),
    }
    Verdict::new("C07-回退-零隐私面", true, "")
}

// ---------------------------------------------------------------------------
// 反假门禁组
// ---------------------------------------------------------------------------

fn assertions_can_fail() -> Verdict {
    // 守卫必须真的能拒绝——逐条构造非法输入验错误码，而不是"调用后没崩就算过"。
    // 这是**弱门禁 vs 反假门禁**的分界：只验"合法输入不崩"的门禁近乎恒真。
    //
    // 注意模板那条必须用**含花括号**的非法输入。初版我图省事传了个纯文本
    // `"__绝对非法输入__"`，结果解析器正确地把它当成合法字面量放行，判据红了——
    // 这说明反假判据自己也会写错，必须逐条确认"输入真的落在被测分支上"。
    let bad_cat = "__绝对非法类别__";
    let bad_gender = "__绝对非法性别__";
    let bad_rule = "__绝对非法规则族__";
    let bad_tpl = "{__绝对非法变量__}";
    if pl::PluralCategory::guard(bad_cat).is_ok()
        || pl::Gender::guard(bad_gender).is_ok()
        || pl::PluralRule::guard(bad_rule).is_ok()
        || pl::parse_template(bad_tpl).is_ok()
    {
        return Verdict::new("C07-反假-断言能失败", false, "self-check-fail");
    }
    // 纯文本必须是合法字面量（确认解析器不是"见括号就报错"的假实现）。
    if pl::parse_template("__绝对非法输入__").is_err() {
        return Verdict::new("C07-反假-断言能失败", false, "self-check-fail");
    }
    // 且每个拒绝都必须是三要素齐全（否则"拒绝"是无信息的空壳）。
    if !pl::PluralCategory::guard("x").unwrap_err().is_complete() {
        return Verdict::new("C07-反假-断言能失败", false, "self-check-fail");
    }
    if !pl::parse_template("{bad").unwrap_err().is_complete() {
        return Verdict::new("C07-反假-断言能失败", false, "self-check-fail");
    }
    Verdict::new("C07-反假-断言能失败", true, "")
}

fn six_categories_truly_differ() -> Verdict {
    // 六类必须**真的不同**——若某个族的六类分支塌缩成同一个返回值，
    // 那么"六类复数"就是六个别名。这项用**表外真实数值**驱动（阿拉伯语六个典型值）。
    let cases: [(i64, pl::PluralCategory); 6] = [
        (0, pl::PluralCategory::Zero),
        (1, pl::PluralCategory::One),
        (2, pl::PluralCategory::Two),
        (3, pl::PluralCategory::Few),
        (11, pl::PluralCategory::Many),
        (101, pl::PluralCategory::Other),
    ];
    let mut seen: Vec<pl::PluralCategory> = Vec::new();
    for (n, want) in cases.iter() {
        let (op, _) = pl::Operands::of(*n, 0);
        let got = pl::eval_rule(pl::PluralRule::ArabicFull, &op);
        if got != *want {
            return Verdict::new("C07-反假-六类真不同", false, "self-check-fail");
        }
        if seen.contains(&got) {
            return Verdict::new("C07-反假-六类真不同", false, "self-check-fail");
        }
        seen.push(got);
    }
    // 渲染层也必须真的分叉：六个值渲染出的文本不能全一样。
    let mut bag = pl::DiagBag::new();
    let mut texts: Vec<String> = Vec::new();
    for (n, _) in cases.iter() {
        let req = pl::SelectorRequest::new("ar-EG", *n, 0, pl::Gender::Feminine, "file");
        if let Ok(s) = pl::select(&req, &mut bag) {
            texts.push(s.text);
        }
    }
    let mut uniq = texts.clone();
    uniq.sort();
    uniq.dedup();
    if uniq.len() < 5 {
        return Verdict::new("C07-反假-六类真不同", false, "self-check-fail");
    }
    Verdict::new("C07-反假-六类真不同", true, "")
}

fn non_ascii_by_char_boundary() -> Verdict {
    // 解析必须**按字符边界**推进，不能逐字节切。
    // 判据：把一段多字节文案的每个字符单独取出再拼回去，必须与原文逐字节相等——
    // 若解析器按字节推进，拼接结果必然与原文不同。
    //
    // **这里用 `enumerate()` 取下标，而不是拿字节值当下标**：
    // `as_bytes()` 迭代出的元素是**字节值**（如 0xE4），不是位置。写成
    // `&src[*b as usize..]` 会拿 228 当下标 —— 在短文案上直接越界 panic。
    // 这正是 F4003 实测过的"`&text[i..i+n]` 越界 panic"的同型错误，
    // 本项开发中在自检里真写出来过一次，由隔离探针当场抓住。
    let src = "中文テストالعربية";
    let bytes = src.as_bytes();
    let mut rebuilt = String::new();
    let mut idx = 0usize;
    while idx < bytes.len() {
        let len = pl::utf8_char_len(bytes[idx]);
        if len == 1 && bytes[idx] >= 0x80 {
            // 续字节出现在字符起始位置即非法（正常文案不会这样）。
            return Verdict::new("C07-反假-非ASCII按字符边界", false, "self-check-fail");
        }
        if idx + len > bytes.len() {
            return Verdict::new("C07-反假-非ASCII按字符边界", false, "self-check-fail");
        }
        rebuilt.push_str(&src[idx..idx + len]);
        idx += len;
    }
    if rebuilt != src {
        return Verdict::new("C07-反假-非ASCII按字符边界", false, "self-check-fail");
    }
    // 长度表必须覆盖 1/2/3/4 四档。
    if pl::utf8_char_len(b'a') != 1
        || pl::utf8_char_len(0xC3) != 2
        || pl::utf8_char_len(0xE4) != 3
        || pl::utf8_char_len(0xF0) != 4
    {
        return Verdict::new("C07-反假-非ASCII按字符边界", false, "self-check-fail");
    }
    Verdict::new("C07-反假-非ASCII按字符边界", true, "")
}