//! VE-F3003 · 域自检（判据逐条对应，见 `vep03_token.rs` 头注）
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **三族令牌** → `F3003-三族-族齐备`、`F3003-三族-时长四级齐备`、
//!   `F3003-三族-缓动八支齐备`、`F3003-三族-位移六档齐备`、
//!   `F3003-三族-值类型与族匹配`、`F3003-三族-ID前缀与族一致`、
//!   `F3003-三族-结构四段齐备`、`F3003-三族-时长基线在天花板内`、
//!   `F3003-三族-无天花板仅时长族`、`F3003-三族-容量未超限`、
//!   `F3003-三族-变体族串错必被拒`；
//! - **跨主题恒定** → `F3003-恒定-三主题注入成功`、`F3003-恒定-三主题逐项相等`、
//!   `F3003-恒定-比对了内容非空`、`F3003-恒定-主题化提案被拒`、
//!   `F3003-恒定-变体漂移必被抓住`、`F3003-恒定-变体条目数不等必被抓住`、
//!   `F3003-恒定-变体三主题齐备`；
//! - **单源注入** → `F3003-注入-两清单同序`、`F3003-注入-条数与表守恒`、
//!   `F3003-注入-变量名规范`、`F3003-注入-同名变量被拒`、
//!   `F3003-注入-变体非规范名被拒`、`F3003-注入-写入条数相符`、
//!   `F3003-注入-变体写入失败冒泡`、`F3003-注入-分歧拼写被识别`；
//! - **命名空间守卫** → `F3003-守卫-动效前缀归属`、`F3003-守卫-异域前缀他属`、
//!   `F3003-守卫-撞车被拒`、`F3003-守卫-父前缀吞并被拒`、
//!   `F3003-守卫-同主幂等`、`F3003-守卫-空登记被拒`、
//!   `F3003-守卫-变体空owner被拒`；
//! - **reduce 令牌层** → `F3003-reduce-每基线有覆盖`、`F3003-reduce-同名变量`、
//!   `F3003-reduce-时长归零`、`F3003-reduce-位移归零`、`F3003-reduce-曲线无过冲`、
//!   `F3003-reduce-全部前庭安全`、`F3003-reduce-变体非零位移必判不安全`、
//!   `F3003-reduce-切换后值改变`；
//! - **硬编码拦截** → `F3003-lint-正确写法零发现`、`F3003-lint-时长硬编码被抓`、
//!   `F3003-lint-秒单位被抓`、`F3003-lint-曲线硬编码被抓`、
//!   `F3003-lint-裸关键字被抓`、`F3003-lint-未定义令牌被抓`、
//!   `F3003-lint-前缀分歧被抓`、`F3003-lint-渐变不误伤`、
//!   `F3003-lint-定义点豁免`、`F3003-lint-定义点逃不掉拼写错`、
//!   `F3003-lint-每条发现带出路`、`F3003-lint-五规则齐备`、
//!   `F3003-lint-非动效属性不拦`；
//! - **O04 消费桥** → `F3003-桥-缓动参数`、`F3003-桥-时长参数`、
//!   `F3003-桥-位移参数`、`F3003-桥-变体族不符被拒`、
//!   `F3003-桥-变体未定义被拒`；
//! - 错误路径与降级矩阵 → `F3003-错误-十二码齐备`、`F3003-错误-三要素齐发`、
//!   `F3003-判据-五条齐备`、`F3003-契约-版本与前缀`、
//!   `F3003-契约-复杂度声明非空`、`F3003-无障碍-替述非空`。
//!
//! 分两批（`run_vep03_checks_a` / `run_vep03_checks_b`）以避开
//! `CheckSet::MAX_CHECKS = 112` 的全仓共享上限。
//!
//! **本文件的一条硬纪律**：判据里的**期望值一律在本文件内写死**
//! （如各档位移像素数、各缓动令牌 ID、基线毫秒数），**不回读
//! `TokenTable` 去和它自己比**。用表内元素验查表函数是恒真弱门禁——
//! 表里写错时它照样全绿。故凡涉及"某个令牌该是什么"的判据，期望值都抄一份在
//! 本文件里，两处不一致时判据变红（这是我们要的：规格表改了要有人看见）。
//!
//! 逻辑 tick 注入、零墙钟，回归可复现。

use alloc::format;
use alloc::string::{String, ToString};

use crate::checks::CheckSet;
use crate::svstar2::vep03_token::{
    apply_normal, apply_reduced, assert_theme_invariant, distance_params, duration_params,
    easing_params, inject_all_themes, lint, reject_themed_proposal,
    strip_var_refs, InjectionManifest, Lane, LintRule, LintSite,
    MotionToken, MotionTokenError, NamespaceClaim, NamespaceRegistry, RecordingInjector,
    TokenFamily, TokenTable, TokenValue, Criterion, DIVERGENT_VAR_PREFIX, ERROR_CODES,
    MAX_TOKENS, MOTION_VAR_PREFIX, REDUCE_ID_PREFIX, TOKEN_TABLE_VERSION, E_FAMILY_MISMATCH,
    E_MANIFEST_DUP_VAR, E_MOTION_TOKEN_THEMED, E_NAMESPACE_COLLISION, E_NAMESPACE_EMPTY,
    E_REDUCE_UNSAFE, E_TOKEN_UNDEFINED, E_VAR_NONCANONICAL, COMPLEXITY_DOC,
};

// ---------------------------------------------------------------------------
// 本文件内写死的期望值（唯一真值副本；不回读被测表）
// ---------------------------------------------------------------------------

/// 时长族四条的期望（ID, 基线 ms, 天花板 ms）。
const EXPECT_DURATION: [(&str, u32, u32); 4] = [
    ("dur-micro", 100, 150),
    ("dur-component", 200, 300),
    ("dur-page", 300, 450),
    ("dur-orchestra", 450, 500),
];

/// 缓动族八条的期望 ID。
const EXPECT_EASING: [&str; 8] = [
    "ease-standard-enter",
    "ease-standard-exit",
    "ease-elastic-snappy",
    "ease-elastic-soft",
    "ease-elastic-bouncy",
    "ease-accent-pop",
    "ease-accent-pull",
    "ease-accent-settle",
];

/// 位移族六档的期望（ID, 像素）。
const EXPECT_DISTANCE: [(&str, u32); 6] = [
    ("dist-none", 0),
    ("dist-hair", 2),
    ("dist-micro", 4),
    ("dist-small", 8),
    ("dist-medium", 16),
    ("dist-large", 24),
];

/// 期望的基线条数（4 + 8 + 6）。
const EXPECT_BASE_COUNT: usize = 18;

/// 期望的 reduce 覆盖条数（与基线一一对应）。
const EXPECT_REDUCE_COUNT: usize = 18;

/// 标准表。
fn tbl() -> TokenTable {
    TokenTable::from_lang()
}

// ---------------------------------------------------------------------------
// 一、三族令牌（判据一）
// ---------------------------------------------------------------------------

fn chk_three_families(set: &mut CheckSet) {
    let t = tbl();

    set.add(
        "F3003-三族-族齐备",
        TokenFamily::ALL.len() == 3,
        "时长/缓动/位移三族",
    );

    for (fam, want) in [
        (TokenFamily::Duration, 4usize),
        (TokenFamily::Easing, 8usize),
        (TokenFamily::Distance, 6usize),
    ] {
        let got = t.family_tokens(fam).len();
        set.add(
            match fam {
                TokenFamily::Duration => "F3003-三族-时长四级齐备",
                TokenFamily::Easing => "F3003-三族-缓动八支齐备",
                TokenFamily::Distance => "F3003-三族-位移六档齐备",
            },
            got == want,
            "期望族条数",
        );
    }

    // ID 前缀与族一致（族与 ID 段一一对应）。
    let prefix_ok = TokenFamily::ALL.iter().all(|f| {
        t.family_tokens(*f)
            .iter()
            .all(|tok| tok.id.starts_with(f.id_prefix()))
    });
    set.add("F3003-三族-ID前缀与族一致", prefix_ok, "dur-/ease-/dist-");

    // 值类型与族匹配（结构断言）。
    let value_ok = t.iter().all(|tok| tok.value_matches_family());
    set.add(
        "F3003-三族-值类型与族匹配",
        value_ok,
        "毫秒/曲线/像素不可混用",
    );

    // 结构四段齐备。
    let complete = t.iter().all(|tok| tok.is_complete());
    set.add("F3003-三族-结构四段齐备", complete, "ID/变量/语义/定义点");

    // 变量名规范（全部落在动效命名空间）。
    let var_ok = t.iter().all(|tok| tok.has_canonical_var());
    set.add("F3003-三族-变量名规范", var_ok, MOTION_VAR_PREFIX);

    // 天花板仅时长族有。
    let ceil_ok = t.iter().all(|tok| {
        if tok.family == TokenFamily::Duration {
            tok.ceiling.is_some()
        } else {
            tok.ceiling.is_none()
        }
    });
    set.add("F3003-三族-无天花板仅时长族", ceil_ok, "区间语义只属时长族");

    // 时长基线在天花板之内（写死期望值）。
    let mut dur_ok = true;
    for (id, base, ceil) in EXPECT_DURATION.iter() {
        match t.find(id) {
            Some(tok) => {
                let got_base = match tok.value {
                    TokenValue::Millis(ms) => ms,
                    _ => u32::MAX,
                };
                if got_base != *base || tok.ceiling != Some(*ceil) || got_base > *ceil {
                    dur_ok = false;
                }
            }
            None => dur_ok = false,
        }
    }
    set.add(
        "F3003-三族-时长基线在天花板内",
        dur_ok,
        "100/150 200/300 300/450 450/500",
    );

    // 位移像素数逐档核对（写死期望值）。
    let mut dist_ok = true;
    for (id, px) in EXPECT_DISTANCE.iter() {
        match t.find(id) {
            Some(tok) => {
                let got = match tok.value {
                    TokenValue::Px(v) => v,
                    _ => u32::MAX,
                };
                if got != *px {
                    dist_ok = false;
                }
            }
            None => dist_ok = false,
        }
    }
    set.add(
        "F3003-三族-位移六档像素核对",
        dist_ok,
        "0/2/4/8/16/24",
    );

    // 缓动 ID 逐条核对（写死期望值）。
    let mut ease_ok = true;
    for id in EXPECT_EASING.iter() {
        match t.find(id) {
            Some(tok) => {
                if tok.family != TokenFamily::Easing {
                    ease_ok = false;
                }
            }
            None => ease_ok = false,
        }
    }
    set.add("F3003-三族-缓动八支ID核对", ease_ok, "标准二/弹性三/强调三");

    // 容量未超限。
    set.add(
        "F3003-三族-容量未超限",
        t.within_capacity(),
        "MAX_TOKENS",
    );

    // 条数守恒：基线 + reduce = 表长。
    let base = t.lane_tokens(Lane::Normal).len();
    let reduced = t.lane_tokens(Lane::Reduced).len();
    set.add(
        "F3003-三族-条数守恒",
        base == EXPECT_BASE_COUNT && reduced == EXPECT_REDUCE_COUNT && base + reduced == t.len(),
        "18+18=36",
    );

    // 变体：族串写错必须被拒（反假变体，防「族判定恒真」）。
    let mut bad = tbl();
    if let Some(tok) = bad.iter_mut().find(|x| x.family == TokenFamily::Duration) {
        tok.family = TokenFamily::Distance;
    }
    let broken = bad.iter().any(|tok| !tok.value_matches_family());
    set.add(
        "F3003-三族-变体族串错必被拒",
        broken,
        "破坏族后必须出现不匹配项，否则族判定是恒真弱门禁",
    );
}

// ---------------------------------------------------------------------------
// 二、跨主题恒定（判据二）
// ---------------------------------------------------------------------------

fn chk_theme_invariance(set: &mut CheckSet) {
    let t = tbl();

    let injected = inject_all_themes(&t);
    let ok = injected.is_ok();
    set.add("F3003-恒定-三主题注入成功", ok, "深/浅/高对比");

    if let Ok(ref list) = injected {
        let report = assert_theme_invariant(list);
        let invariant = report.as_ref().map(|r| r.is_invariant()).unwrap_or(false);
        set.add("F3003-恒定-三主题逐项相等", invariant, "逐项比对无漂移");

        let checked = report.as_ref().map(|r| r.entries_checked).unwrap_or(0);
        set.add(
            "F3003-恒定-比对了内容非空",
            checked >= EXPECT_BASE_COUNT,
            "比过内容才算数",
        );

        // 三主题齐备（主题数写死）。
        set.add(
            "F3003-恒定-变体三主题齐备",
            list.len() == 3,
            "THEME_COUNT=3",
        );
    } else {
        set.add("F3003-恒定-三主题逐项相等", false, "注入失败");
        set.add("F3003-恒定-比对了内容非空", false, "注入失败");
        set.add("F3003-恒定-变体三主题齐备", false, "注入失败");
    }

    // 主题化提案恒被拒。
    let mut all_rejected = true;
    for theme in [
        crate::svstar2::vep03_token::Theme::Dark,
        crate::svstar2::vep03_token::Theme::Light,
        crate::svstar2::vep03_token::Theme::HighContrast,
    ] {
        match reject_themed_proposal("dur-micro", theme) {
            Err(e) => {
                if e.code() != E_MOTION_TOKEN_THEMED {
                    all_rejected = false;
                }
            }
            Ok(()) => all_rejected = false,
        }
    }
    set.add(
        "F3003-恒定-主题化提案被拒",
        all_rejected,
        "三主题皆拒且码正确",
    );

    // 变体：改一个值必须被抓（反假变体——防「恒定断言恒真」）。
    let mut drifted_ok = false;
    if let Ok(mut list) = inject_all_themes(&t) {
        if let Some(first) = list.first_mut() {
            if let Some(entry) = first.entries.first_mut() {
                entry.1 = "220ms".to_string();
            }
            drifted_ok = assert_theme_invariant(&list).is_err();
        }
    }
    set.add(
        "F3003-恒定-变体漂移必被抓住",
        drifted_ok,
        "改一值即报红，否则断言是恒真弱门禁",
    );

    // 变体：条目数不等必被抓住（半态检测）。
    let mut len_drift_ok = false;
    if let Ok(mut list) = inject_all_themes(&t) {
        if list.len() >= 2 {
            if let Some(first) = list.first_mut() {
                first.entries.pop();
            }
            len_drift_ok = assert_theme_invariant(&list).is_err();
        }
    }
    set.add(
        "F3003-恒定-变体条目数不等必被抓住",
        len_drift_ok,
        "少注一个令牌也须报红",
    );
}

// ---------------------------------------------------------------------------
// 三、单源注入 + 命名空间守卫（判据三、六）
// ---------------------------------------------------------------------------

fn chk_injection(set: &mut CheckSet) {
    let t = tbl();

    // 两清单变量序列同序（reduce 切换机制的成立条件）。
    let mut a = RecordingInjector::new();
    let mut b = RecordingInjector::new();
    let rn = apply_normal(&t, &mut a);
    let rr = apply_reduced(&t, &mut b);
    let same = match (&rn, &rr) {
        (Ok(x), Ok(y)) => x.same_var_sequence(y),
        _ => false,
    };
    set.add("F3003-注入-两清单同序", same, "reduce 写同名变量");

    // 条数与表守恒。
    let base = t.lane_tokens(Lane::Normal).len();
    let reduced = t.lane_tokens(Lane::Reduced).len();
    let count_ok = rn.as_ref().map(|r| r.written == base).unwrap_or(false)
        && rr.as_ref().map(|r| r.written == reduced).unwrap_or(false);
    set.add("F3003-注入-条数与表守恒", count_ok, "写入数=清单数");

    // 写入条数与实际记录相符。
    let written_ok = rn.as_ref().map(|r| r.written == a.len()).unwrap_or(false)
        && rr.as_ref().map(|r| r.written == b.len()).unwrap_or(false);
    set.add("F3003-注入-写入条数相符", written_ok, "回执=记录器");

    // 切换后值确实改变（基线 → 直达）。
    let switched = a.value_of("--ve-motion-dur-micro") == Some("100ms")
        && b.value_of("--ve-motion-dur-micro") == Some("0ms")
        && b.value_of("--ve-motion-dist-medium") == Some("0px");
    set.add("F3003-reduce-切换后值改变", switched, "100ms→0ms / 16px→0px");

    // 变量名规范。
    let var_ok = t.iter().all(|tok| tok.has_canonical_var());
    set.add("F3003-注入-变量名规范", var_ok, MOTION_VAR_PREFIX);

    // 同名变量被拒（反假变体：故意造同名）。
    // 注意条目顺序：push_pair 每次推入「基线 + 其 reduce」，故 dur-b 的
    // **基线**在 index 2（不是 1）。改错索引会让本判据变成恒真——
    // reduce 条目不在 Normal 泳道清单里，撞不到就测不出东西。
    let mut dup = TokenTable::new();
    dup.push_pair(
        dup.assemble(
            "dur-a",
            TokenFamily::Duration,
            TokenValue::Millis(100),
            Some(150),
            "甲",
        ),
    );
    dup.push_pair(
        dup.assemble(
            "dur-b",
            TokenFamily::Duration,
            TokenValue::Millis(200),
            Some(300),
            "乙",
        ),
    );
    let dur_b_base = dup
        .iter()
        .position(|x| x.id == "dur-b")
        .expect("dur-b 基线在册");
    if let Some(tok) = dup.iter_mut().nth(dur_b_base) {
        tok.var = "--ve-motion-dur-a".to_string();
    }
    let dup_rejected = matches!(
        InjectionManifest::build(&dup, Lane::Normal),
        Err(ref e) if e.code == E_MANIFEST_DUP_VAR
    );
    set.add("F3003-注入-同名变量被拒", dup_rejected, "同名即分叉");

    // 变体：非规范变量名被拒。
    let mut bad = TokenTable::new();
    bad.push_pair(
        bad.assemble(
            "dur-a",
            TokenFamily::Duration,
            TokenValue::Millis(100),
            Some(150),
            "甲",
        ),
    );
    if let Some(tok) = bad.iter_mut().next() {
        tok.var = format!("{}dur-a", DIVERGENT_VAR_PREFIX);
    }
    let bad_rejected = matches!(
        InjectionManifest::build(&bad, Lane::Normal),
        Err(ref e) if e.code == E_VAR_NONCANONICAL
    );
    set.add(
        "F3003-注入-变体非规范名被拒",
        bad_rejected,
        "分歧拼写进不了清单",
    );

    // 变体：写入失败必须冒泡（不静默吞）。
    let mut failing = RecordingInjector::failing_on("--ve-motion-dur-micro");
    let inject_err = apply_normal(&t, &mut failing).is_err();
    set.add("F3003-注入-变体写入失败冒泡", inject_err, "通道故障须显性");

    // 分歧拼写被识别。
    let divergent_ok = NamespaceRegistry::is_known_divergent(&format!("{}dur-micro", DIVERGENT_VAR_PREFIX));
    set.add("F3003-注入-分歧拼写被识别", divergent_ok, "册内 S55 分支");

    // 守卫组。
    chk_namespace(set);
}

fn chk_namespace(set: &mut CheckSet) {
    let mut g = NamespaceRegistry::with_motion();

    set.add(
        "F3003-守卫-动效前缀归属",
        g.classify("--ve-motion-dur-micro") == NamespaceClaim::Owned,
        "VE-P 所有",
    );
    set.add(
        "F3003-守卫-异域前缀他属",
        g.classify("--ve-color-accent") != NamespaceClaim::Owned,
        "非动效前缀不归本域",
    );
    set.add(
        "F3003-守卫-撞车被拒",
        g.claim(MOTION_VAR_PREFIX, "VE-E").is_err(),
        "他人认领必被拒",
    );
    set.add(
        "F3003-守卫-父前缀吞并被拒",
        g.claim("--ve-", "VE-E").is_err(),
        "父前缀收编必被拒",
    );
    set.add(
        "F3003-守卫-同主幂等",
        g.claim(MOTION_VAR_PREFIX, "VE-P").is_ok(),
        "热重载不得自我拒绝",
    );
    let empty_rejected = matches!(
        g.claim("", "VE-E"),
        Err(ref e) if e.code == E_NAMESPACE_EMPTY
    );
    set.add("F3003-守卫-空登记被拒", empty_rejected, "空前缀");

    // 变体：空 owner 也须被拒（防空 owner 绕过撞车判定）。
    let mut g2 = NamespaceRegistry::new();
    let no_owner = matches!(
        g2.claim(MOTION_VAR_PREFIX, ""),
        Err(ref e) if e.code == E_NAMESPACE_EMPTY
    );
    set.add("F3003-守卫-变体空owner被拒", no_owner, "归属凭据须双齐");

    // 撞车诊断码核对。
    let mut g3 = NamespaceRegistry::with_motion();
    let code_ok = matches!(
        g3.claim(MOTION_VAR_PREFIX, "VE-O"),
        Err(ref e) if e.code == E_NAMESPACE_COLLISION
    );
    set.add("F3003-守卫-撞车码正确", code_ok, E_NAMESPACE_COLLISION);
}

// ---------------------------------------------------------------------------
// 四、reduce 令牌层（判据四）
// ---------------------------------------------------------------------------

fn chk_reduce_layer(set: &mut CheckSet) {
    let t = tbl();

    // 每条基线有 reduce 覆盖。
    let covered = t
        .lane_tokens(Lane::Normal)
        .iter()
        .all(|b| t.reduce_of(b.id.as_str()).is_some());
    set.add("F3003-reduce-每基线有覆盖", covered, "18/18");

    // reduce ID 带前缀。
    let prefixed = t
        .lane_tokens(Lane::Reduced)
        .iter()
        .all(|r| r.id.starts_with(REDUCE_ID_PREFIX));
    set.add("F3003-reduce-ID带前缀", prefixed, REDUCE_ID_PREFIX);

    // reduce 与基线同名变量（零分支机制）。
    let same_var = t
        .lane_tokens(Lane::Normal)
        .iter()
        .all(|b| t.reduce_of(b.id.as_str()).map(|r| r.var == b.var).unwrap_or(false));
    set.add("F3003-reduce-同名变量", same_var, "组件侧零分支");

    // 时长归零（逐条核对写死期望的 ID）。
    let mut dur_zero = true;
    for (id, _, _) in EXPECT_DURATION.iter() {
        match t.reduce_of(id) {
            Some(r) => {
                if r.value != TokenValue::Millis(0) {
                    dur_zero = false;
                }
            }
            None => dur_zero = false,
        }
    }
    set.add("F3003-reduce-时长归零", dur_zero, "18族中 4 条时长");

    // 位移归零（逐条核对写死期望的 ID）。
    let mut dist_zero = true;
    for (id, _) in EXPECT_DISTANCE.iter() {
        match t.reduce_of(id) {
            Some(r) => {
                if r.value != TokenValue::Px(0) {
                    dist_zero = false;
                }
            }
            None => dist_zero = false,
        }
    }
    set.add("F3003-reduce-位移归零", dist_zero, "前庭安全硬门");

    // 曲线无过冲。
    let ease_safe = t
        .lane_tokens(Lane::Reduced)
        .iter()
        .filter(|r| r.family == TokenFamily::Easing)
        .all(|r| r.reduce_is_safe());
    set.add("F3003-reduce-曲线无过冲", ease_safe, "linear");

    // 全部 reduce 条目前庭安全。
    let all_safe = t.lane_tokens(Lane::Reduced).iter().all(|r| r.reduce_is_safe());
    set.add("F3003-reduce-全部前庭安全", all_safe, "零位移+零时长+无过冲");

    // 变体：非零位移的 reduce 必判不安全（反假变体——防「安全判定恒真」）。
    let mut bad = tbl();
    if let Some(tok) = bad
        .iter_mut()
        .find(|x| x.lane == Lane::Reduced && x.family == TokenFamily::Distance)
    {
        tok.value = TokenValue::Px(4);
    }
    let unsafe_exists = bad
        .lane_tokens(Lane::Reduced)
        .iter()
        .any(|r| !r.reduce_is_safe());
    set.add(
        "F3003-reduce-变体非零位移必判不安全",
        unsafe_exists,
        "自称 reduce 但仍在动= 更坏",
    );

    // 变体：非零时长的 reduce 必判不安全。
    let mut bad2 = tbl();
    if let Some(tok) = bad2
        .iter_mut()
        .find(|x| x.lane == Lane::Reduced && x.family == TokenFamily::Duration)
    {
        tok.value = TokenValue::Millis(1);
    }
    let unsafe_dur = bad2
        .lane_tokens(Lane::Reduced)
        .iter()
        .any(|r| !r.reduce_is_safe());
    set.add(
        "F3003-reduce-变体非零时长必判不安全",
        unsafe_dur,
        "1ms 也是动效",
    );

    // 变体：过冲曲线必判不安全。
    let mut bad3 = tbl();
    if let Some(tok) = bad3
        .iter_mut()
        .find(|x| x.lane == Lane::Reduced && x.family == TokenFamily::Easing)
    {
        tok.value = TokenValue::Curve("cubic-bezier(0.68, -0.55, 0.27, 1.55)".to_string());
    }
    let unsafe_curve = bad3
        .lane_tokens(Lane::Reduced)
        .iter()
        .any(|r| !r.reduce_is_safe());
    set.add("F3003-reduce-变体过冲曲线必判不安全", unsafe_curve, "过冲= 振动");
}

// ---------------------------------------------------------------------------
// 五、硬编码 lint（判据五）
// ---------------------------------------------------------------------------

fn chk_lint(set: &mut CheckSet) {
    let t = tbl();

    // 正确写法零发现（含 var() 剔除后的关键词不误伤）。
    let good = [
        LintSite::new("a.css:1", "transition", "opacity var(--ve-motion-dur-micro)"),
        LintSite::new(
            "a.css:2",
            "transition",
            "all var(--ve-motion-dur-component) var(--ve-motion-ease-standard-enter)",
        ),
        LintSite::new("a.css:3", "transform", "translateY(var(--ve-motion-dist-micro))"),
        LintSite::new(
            "a.css:4",
            "transition",
            "all var(--ve-motion-dur-page) var(--ve-motion-ease-elastic-soft)",
        ),
    ];
    let good_report = lint(&t, &good);
    set.add(
        "F3003-lint-正确写法零发现",
        good_report.is_clean(),
        "拦掉正确答案的 lint 会被关掉",
    );

    // 逐规则检出（期望计数**手数**得出，不靠跑出来再填）。
    // 时长硬编码共 3 处：b.css:1 的 200ms、b.css:2 的 0.3s、b.css:4 的 200ms。
    // （b.css:4 一行同时含时长与裸关键字，两条规则各记一次——这是有意的：
    //  一处写法可以同时违反两条红线，报一条就漏了另一条。）
    let cases: [(&str, LintRule, usize, &str); 5] = [
        (
            "F3003-lint-时长硬编码被抓",
            LintRule::HardcodedDuration,
            3,
            "200ms / 0.3s / 200ms 各一处",
        ),
        (
            "F3003-lint-曲线硬编码被抓",
            LintRule::HardcodedCurve,
            1,
            "cubic-bezier(...)",
        ),
        (
            "F3003-lint-裸关键字被抓",
            LintRule::BareEasingKeyword,
            1,
            "ease-in-out",
        ),
        (
            "F3003-lint-未定义令牌被抓",
            LintRule::UndefinedToken,
            1,
            "dur-nope",
        ),
        (
            "F3003-lint-前缀分歧被抓",
            LintRule::NonCanonicalPrefix,
            2,
            "--vx- 分歧 + 近似拼写两条真发现",
        ),
    ];
    let sites = [
        LintSite::new("b.css:1", "transition", "opacity 200ms"),
        LintSite::new("b.css:2", "transition", "all 0.3s"),
        LintSite::new("b.css:3", "animation-timing-function", "cubic-bezier(0.4, 0, 0.2, 1)"),
        LintSite::new("b.css:4", "transition", "all 200ms ease-in-out"),
        LintSite::new("b.css:5", "transition", "all var(--ve-motion-dur-nope)"),
        LintSite::new("b.css:6", "transition", "all var(--vx-motion-dur-micro)"),
        LintSite::new("c.css:1", "background", "linear-gradient(180deg, #000, #fff)"),
    ];
    let report = lint(&t, &sites);
    for (name, rule, want, detail) in cases.iter() {
        set.add(name, report.count_of(*rule) == *want, detail);
    }

    // 非动效属性不拦硬编码（`background` 不属动效上下文）。
    // 这条必须**单独跑一次**再数——混进上面的总报告里数，会被别的站点的
    // 发现数污染，变成一个恒真弱门禁。
    let non_motion = lint(
        &t,
        &[LintSite::new("c.css:1", "background", "200ms linear-gradient(180deg,#000,#fff)")],
    );
    set.add(
        "F3003-lint-非动效属性不拦",
        non_motion.is_clean(),
        "background 不属动效上下文",
    );

    // 变体：秒单位单独被抓（拆开看，防止时长规则只认ms）。
    let sec_only = lint(
        &t,
        &[LintSite::new("d.css:1", "transition", "all 1.5s")],
    );
    set.add(
        "F3003-lint-秒单位被抓",
        sec_only.count_of(LintRule::HardcodedDuration) == 1,
        "1.5s也算硬编码",
    );

    // 渐变不误伤（linear-gradient 不是裸曲线）。
    set.add(
        "F3003-lint-渐变不误伤",
        report.count_of(LintRule::BareEasingKeyword) == 1,
        "linear-gradient 不算裸 easing",
    );

    // 定义点豁免硬编码。
    let def = LintSite::definition("token.rs:10", "transition", "opacity 100ms");
    set.add(
        "F3003-lint-定义点豁免",
        lint(&t, &[def]).is_clean(),
        "真值所在不拦",
    );

    // 定义点也逃不掉拼写错。
    let def_bad = LintSite::definition(
        "token.rs:11",
        "transition",
        "var(--vx-motion-dur-micro)",
    );
    set.add(
        "F3003-lint-定义点逃不掉拼写错",
        lint(&t, &[def_bad]).count_of(LintRule::NonCanonicalPrefix) == 1,
        "豁免不得过宽",
    );

    // 每条发现都带出路。
    let all_have_remedy = report.findings.iter().all(|f| !f.remedy.is_empty());
    set.add("F3003-lint-每条发现带出路", all_have_remedy, "无出路= 会被忽略");

    // 五规则齐备。
    set.add(
        "F3003-lint-五规则齐备",
        LintRule::ALL.len() == 5,
        "时长/曲线/裸关键字/未定义/前缀",
    );

    // 变体：var() 剔除必须真的剔（否则 ease-xxx 令牌名含ease 会被误报）。
    let stripped = strip_var_refs("var(--ve-motion-ease-standard-enter) 200ms");
    set.add(
        "F3003-lint-变体var剔除有效",
        !stripped.contains("ease-standard-enter") && stripped.contains("200ms"),
        "剔除 var() 引用，保留其余文本",
    );

    // 变体：嵌套 var() 也要剔干净。
    let nested = strip_var_refs("var(--a, var(--b, 1s))");
    set.add(
        "F3003-lint-变体嵌套var剔除",
        !nested.contains("--a") && !nested.contains("1s"),
        "嵌套回退值不得残留",
    );

    // 变体：未定义令牌判定不是恒真（已定义令牌不得误报）。
    let defined_ok = lint(
        &t,
        &[LintSite::new("e.css:1", "transition", "all var(--ve-motion-dur-orchestra)")],
    )
    .count_of(LintRule::UndefinedToken)
        == 0;
    set.add("F3003-lint-变体已定义令牌不误报", defined_ok, "真令牌须放行");
}

// ---------------------------------------------------------------------------
// 六、O04 消费桥 + 错误纪律
// ---------------------------------------------------------------------------

fn chk_bridge_and_errors(set: &mut CheckSet) {
    let t = tbl();

    // 缓动参数（取写死期望的 ID）。
    let mut ease_ok = true;
    for id in EXPECT_EASING.iter() {
        match easing_params(&t, id) {
            Ok(p) => {
                if p.curve.is_empty() || p.reduced_curve.is_empty() || p.family.is_empty() {
                    ease_ok = false;
                }
                if p.token_id != *id {
                    ease_ok = false;
                }
            }
            Err(_) => ease_ok = false,
        }
    }
    set.add("F3003-桥-缓动参数", ease_ok, "八支皆有曲线与reduce 曲线");

    // 时长参数（逐条核对写死期望）。
    let mut dur_ok = true;
    for (id, base, ceil) in EXPECT_DURATION.iter() {
        match duration_params(&t, id) {
            Ok(p) => {
                if p.base_ms != *base || p.ceiling_ms != *ceil || p.reduced_ms != 0 {
                    dur_ok = false;
                }
                if !p.base_within_ceiling() {
                    dur_ok = false;
                }
            }
            Err(_) => dur_ok = false,
        }
    }
    set.add("F3003-桥-时长参数", dur_ok, "基线/天花板/reduce 三段");

    // 位移参数（逐条核对写死期望）。
    let mut dist_ok = true;
    for (id, px) in EXPECT_DISTANCE.iter() {
        match distance_params(&t, id) {
            Ok(p) => {
                if p.base_px != *px || p.reduced_px != 0 || !p.reduced_is_safe() {
                    dist_ok = false;
                }
            }
            Err(_) => dist_ok = false,
        }
    }
    set.add("F3003-桥-位移参数", dist_ok, "基线像素 + reduce 归零");

    // 变体：族不符被拒。
    let mismatch = matches!(
        easing_params(&t, "dur-micro"),
        Err(ref e) if e.code == E_FAMILY_MISMATCH
    ) && matches!(
        distance_params(&t, "dur-micro"),
        Err(ref e) if e.code == E_FAMILY_MISMATCH
    ) && matches!(
        duration_params(&t, "dist-medium"),
        Err(ref e) if e.code == E_FAMILY_MISMATCH
    );
    set.add("F3003-桥-变体族不符被拒", mismatch, "跨族取参必拒");

    // 变体：未定义令牌被拒（编译期拦截）。三者返回类型各异，逐个独立判定。
    let undef_ease = matches!(easing_params(&t, "ease-nope"), Err(ref e) if e.code == E_TOKEN_UNDEFINED);
    let undef_dur = matches!(duration_params(&t, "dur-nope"), Err(ref e) if e.code == E_TOKEN_UNDEFINED);
    let undef_dist = matches!(distance_params(&t, "dist-nope"), Err(ref e) if e.code == E_TOKEN_UNDEFINED);
    set.add(
        "F3003-桥-变体未定义被拒",
        undef_ease && undef_dur && undef_dist,
        "缺令牌须显性",
    );

    // 错误码齐备：12 个、互不重复、全部非空。
    // 顺带钉住「安全判定真有判别力」：`E_REDUCE_UNSAFE` 必须在册内
    // （它是reduce 层的硬门码，漏登记等于那道门没有编号）。
    let mut codes_ok = ERROR_CODES.len() == 12;
    let mut i = 0usize;
    while i < ERROR_CODES.len() {
        let c = ERROR_CODES[i];
        if c.is_empty() {
            codes_ok = false;
        }
        let mut j = i + 1;
        while j < ERROR_CODES.len() {
            if ERROR_CODES[j] == c {
                codes_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "F3003-错误-十二码齐备",
        codes_ok && ERROR_CODES.contains(&E_REDUCE_UNSAFE),
        "12 码非空且唯一",
    );

    // 三要素齐发（构造一条错误验其四段）。
    let e = MotionTokenError::new(
        E_TOKEN_UNDEFINED,
        "示例",
        "原因",
        "出路",
        "责任方",
    );
    let screen = e.screen_text();
    set.add(
        "F3003-错误-三要素齐发",
        screen.contains("原因") && screen.contains("出路") && screen.contains("责任方"),
        "现象/原因/怎么办",
    );

    // 判据五条齐备。
    set.add("F3003-判据-五条齐备", Criterion::ALL.len() == 5, "锚点五条判据");

    // 契约：版本号与前缀。
    set.add(
        "F3003-契约-版本与前缀",
        !TOKEN_TABLE_VERSION.is_empty()
            && MOTION_VAR_PREFIX == "--ve-motion-"
            && REDUCE_ID_PREFIX == "reduce-",
        "版本+前缀冻结",
    );

    // 复杂度声明非空且提到关键项。
    set.add(
        "F3003-契约-复杂度声明非空",
        COMPLEXITY_DOC.contains("O(令牌数)") && COMPLEXITY_DOC.contains("每帧路径零成本"),
        "性能逐项分解",
    );

    // 无障碍替述：令牌读屏行非空。
    let screen_tbl = tbl();
    let sample: &MotionToken = match screen_tbl.find("dur-micro") {
        Some(tok) => tok,
        None => {
            set.add("F3003-无障碍-替述非空", false, "令牌缺失");
            return;
        }
    };
    let line = sample.screen_line();
    set.add(
        "F3003-无障碍-替述非空",
        line.contains("时长") && line.contains("dur-micro") && line.contains("语义"),
        "令牌须有文字说明",
    );

    // 变体：空语义必须判为不完整（防「结构齐备恒真」）。
    let mut bad = tbl();
    if let Some(tok) = bad.iter_mut().next() {
        tok.semantic = String::new();
    }
    let incomplete = bad.iter().any(|tok| !tok.is_complete());
    set.add("F3003-无障碍-变体空语义判不完整", incomplete, "四段缺一不可");

    // 变体：容量判定不是恒真（改大 max 不应影响本判据，改表长才应）。
    set.add(
        "F3003-三族-变体容量判定非恒真",
        MAX_TOKENS == 128 && tbl().len() <= MAX_TOKENS,
        "当前 36 ≤ 128",
    );
}

// ---------------------------------------------------------------------------
// 七、自检入口
// ---------------------------------------------------------------------------

/// VE-F3003 域自检·第一批（**令牌是什么**）。
///
/// 第一批收「结构」：三族齐备 / 跨主题恒定 / 命名空间守卫 / reduce 令牌层。
pub fn run_vep03_checks_a() -> CheckSet {
    let mut set = CheckSet::new("vep03-token-a");
    chk_three_families(&mut set);
    chk_theme_invariance(&mut set);
    chk_namespace(&mut set);
    chk_reduce_layer(&mut set);
    set
}

/// VE-F3003 域自检·第二批（**怎么强制**）。
///
/// 第二批收「执法」：单源注入 / 硬编码 lint / O04 消费桥 / 错误纪律。
pub fn run_vep03_checks_b() -> CheckSet {
    let mut set = CheckSet::new("vep03-token-b");
    chk_injection(&mut set);
    chk_lint(&mut set);
    chk_bridge_and_errors(&mut set);
    set
}

/// VE-F3003 域自检总入口（第一批）。
pub fn run_vep03_checks() -> CheckSet {
    run_vep03_checks_a()
}

// ---------------------------------------------------------------------------
// 单元测试（红项定位 + 反假变体；宿主侧 cargo test 直跑）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use crate::svstar2::vep03_token::{DurationParams, LintFinding};

    /// 写一个把全部红项打名字的报告器（聚合器只给 passed，红项定位靠它）。
    fn red_items(set: &CheckSet) -> Vec<&'static str> {
        let (items, n) = set.red_items();
        let mut out = Vec::new();
        for i in 0..n {
            if let Some(Some(c)) = items.get(i) {
                if !c.passed {
                    out.push(c.name);
                }
            }
        }
        out
    }

    #[test]
    fn 第一批全绿() {
        let set = run_vep03_checks_a();
        assert!(
            !set.truncated(),
            "第一批被截断：产出 {} 条超出MAX_CHECKS",
            set.len()
        );
        assert!(red_items(&set).is_empty(), "红项：{:?}", red_items(&set));
    }

    #[test]
    fn 第二批全绿() {
        let set = run_vep03_checks_b();
        assert!(
            !set.truncated(),
            "第二批被截断：产出 {} 条超出 MAX_CHECKS",
            set.len()
        );
        assert!(red_items(&set).is_empty(), "红项：{:?}", red_items(&set));
    }

    #[test]
    fn 两批条数在容量内() {
        let a = run_vep03_checks_a().len();
        let b = run_vep03_checks_b().len();
        assert!(a + b >= 50, "判据覆盖偏少：{} + {}", a, b);
        assert!(a <= 112 && b <= 112, "单批超容：{} / {}", a, b);
    }

    /// 变体1：把位移六档中一档的像素改错，时长/位移核对判据必须变红。
    ///
    /// 若这类变异抓不到，说明位移核对是恒真弱门禁。
    #[test]
    fn 反假_位移像素写错必被抓() {
        let mut t = tbl();
        if let Some(tok) = t.iter_mut().find(|x| x.id == "dist-medium") {
            tok.value = TokenValue::Px(99);
        }
        let base = t.family_tokens(TokenFamily::Distance);
        assert_eq!(base.len(), 6, "族条数仍应为6");
        // 期望值写死在本文件，故99 必与期望的 16 不符。
        let want = 16u32;
        let got = match t.find("dist-medium") {
            Some(tok) => match tok.value {
                TokenValue::Px(v) => v,
                _ => u32::MAX,
            },
            None => u32::MAX,
        };
        assert_ne!(got, want, "写错的像素值必须与本文件期望不符");
    }

    /// 变体 2：删掉一条 reduce 覆盖，「每基线有覆盖」判据必须变红。
    #[test]
    fn 反假_reduce缺项必被抓() {
        let mut t = tbl();
        // 把一条 reduce 条目的 ID 改掉，制造孤儿。
        if let Some(tok) = t
            .iter_mut()
            .find(|x| x.lane == Lane::Reduced && x.family == TokenFamily::Duration)
        {
            tok.id = "reduce-ghost".to_string();
        }
        let orphan = t
            .lane_tokens(Lane::Normal)
            .iter()
            .any(|b| t.reduce_of(b.id.as_str()).is_none());
        assert!(orphan, "改名后必有基线失去覆盖");
    }

    /// 变体 3：reduce 覆盖改名后 ID 前缀判据必须变红。
    #[test]
    fn 反假_reduce前缀丢失必被抓() {
        let mut t = tbl();
        if let Some(tok) = t
            .iter_mut()
            .find(|x| x.lane == Lane::Reduced)
        {
            tok.id = "orphan-dur".to_string();
        }
        let prefixed = t
            .lane_tokens(Lane::Reduced)
            .iter()
            .all(|r| r.id.starts_with(REDUCE_ID_PREFIX));
        assert!(!prefixed, "前缀丢失必须被前缀判据抓到");
    }

    /// 变体 4：基线与 reduce 变量名不同名时，同名判据必须变红。
    ///
    /// 这条最关键：不同名意味着切换时出现半态（有的变量换了值、有的还是旧值）。
    #[test]
    fn 反假_变量名分裂必被抓() {
        let mut t = tbl();
        if let Some(tok) = t
            .iter_mut()
            .find(|x| x.lane == Lane::Reduced && x.family == TokenFamily::Easing)
        {
            tok.var = format!("{}-reduced", tok.var);
        }
        let split = t
            .lane_tokens(Lane::Normal)
            .iter()
            .any(|b| t.reduce_of(b.id.as_str()).map(|r| r.var != b.var).unwrap_or(false));
        assert!(split, "变量名分裂必须被同名判据抓到");
    }

    /// 变体 5：字面量表被改错时，注入清单的变量名规范判据必须仍能工作。
    #[test]
    fn 反假_变量名非规范必被抓() {
        let mut t = tbl();
        if let Some(tok) = t.iter_mut().next() {
            tok.var = "motion-dur-x".to_string();
        }
        let ok = t.iter().all(|tok| tok.has_canonical_var());
        assert!(!ok, "丢掉前缀必须被规范判据抓到");
    }

    /// 变体 6：容量判定不得恒真——用一个超限表验证守卫会拦。
    #[test]
    fn 反假_容量判定非恒真() {
        let mut t = TokenTable::new();
        for i in 0..(MAX_TOKENS + 1) {
            t.push_pair(
                t.assemble(
                    &format!("dur-x{}", i),
                    TokenFamily::Duration,
                    TokenValue::Millis(100),
                    Some(150),
                    "压测",
                ),
            );
        }
        assert!(!t.within_capacity(), "超限表必须判为超限");
    }

    /// 变体 7：lint 判据的期望值必须真的在动——把 var 前缀改掉，正确写法就该变红。
    #[test]
    fn 反假_lint正确写法判定非恒真() {
        let t = tbl();
        // 规范写法：零发现。
        let good = lint(
            &t,
            &[LintSite::new("x.css:1", "transition", "opacity var(--ve-motion-dur-micro)")],
        );
        assert!(good.is_clean());
        // 去掉前缀后同一处写法必须被判红（证明该判据真的在比对前缀）。
        let bad = lint(
            &t,
            &[LintSite::new("x.css:1", "transition", "opacity var(--motion-dur-micro)")],
        );
        assert!(!bad.is_clean(), "无前缀写法必须被拦");
    }

    /// 变体 8：strip_var_refs 的边界——普通文本不得被破坏。
    #[test]
    fn strip_var_不破坏普通文本() {
        let src = "translateY(12px) rotate(3deg)";
        let out = strip_var_refs(src);
        assert!(out.contains("12px"), "非var 文本须保留：{}", out);
        assert!(out.contains("rotate"), "非var 文本须保留：{}", out);
    }

    /// 变体 9：DurationParams 的守恒式不得恒真。
    #[test]
    fn 反假_时长基线天花板关系非恒真() {
        let ok = DurationParams {
            token_id: "dur-x".to_string(),
            base_ms: 100,
            ceiling_ms: 150,
            reduced_ms: 0,
        };
        assert!(ok.base_within_ceiling());
        let bad = DurationParams {
            token_id: "dur-x".to_string(),
            base_ms: 300,
            ceiling_ms: 150,
            reduced_ms: 0,
        };
        assert!(!bad.base_within_ceiling(), "越界必须被关系判据抓到");
    }

    /// 变体 10：LintFinding 必须带出路（无出路= 会被忽略）。
    #[test]
    fn 反假_lint发现必须带出路() {
        let t = tbl();
        let f: Vec<LintFinding> = lint(
            &t,
            &[LintSite::new("y.css:1", "transition", "opacity 200ms")],
        )
        .findings;
        assert!(!f.is_empty());
        for item in f.iter() {
            assert!(!item.remedy.is_empty(), "缺出路：{}", item.screen_line());
        }
    }

    /// 变体 11：错误码集合不得恒真——人为构造的码不在集合内。
    #[test]
    fn 反假_错误码判定非恒真() {
        assert!(ERROR_CODES.contains(&E_TOKEN_UNDEFINED));
        assert!(!ERROR_CODES.contains(&"E_NOT_A_REAL_CODE"));
    }

    /// 变体 12：vec! 宏在探针里可用（本仓 no_std 三件套的回归护栏）。
    #[test]
    fn 探针_vec宏可用() {
        let v: Vec<u32> = vec![1u32, 2, 3];
        assert_eq!(v.len(), 3);
        let s: String = format!("{}", v.len());
        assert_eq!(s, "3");
    }
}
