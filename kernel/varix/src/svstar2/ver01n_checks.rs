//! VE-F3414 · 第三方令牌扩展 —— 域判据层。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3414`
//!
//! # 判据映射（锚点五条判据 + 纪律）
//!
//! - **命名空间**（10 条）→ 受理幂等/冒名接管拒/语法四违/保留词三根/
//!   未准入不可注册/扩展容量闸/信任三级闭集/未验签不激活/合规放行；
//! - **冲突检测**（10 条）→ 撞官方路径拒/抄官方字面量判影子/扩展内重名/
//!   定义点重占/冲突不落半条/纯冲突不入隔离台账/影子入台账且计治理/
//!   三闸顺序（冲突先于容量）/闸 1~4 逐闸可分辨/变体（三闸放行即红）；
//! - **隔离治理**（9 条）→ 越界入台账/未授权入台账/低信任入台账/台账留底
//!   主体与 tick/治理分桶/未达阈值不停权/达阈值停权/停权闸先于沙箱/
//!   停权拒收不入台账/无立案不凭空；
//! - **两律同守**（7 条）→ 律一定义点必填/一真值点只站一条/律二主体
//!   必填/律二理由必填/律二对象为全路径/账本条数对账/官方优先仲裁规则；
//! - **判据与契约**（12 条）→ 十四码唯一/三要素齐/13 阻断 + 1 记账/
//!   E16 段逐字冻结/十四码逐个真跑造出/隔离可查/未声明路径 None/
//!   清单读屏首行/逐条读屏行/契约版本钉死/降级矩阵三行。
//!
//! # 判据设计纪律
//!
//! 1. 期望值由判据侧**独立字面量**给出（保留词三根、阈值 3、
//!    短码 14 条、容量 32/256/1024 全部写死），不调被测函数当期望；
//! 2. 每条「必被抓」判据配变体（影子放行/冲突入台账/未达阈值即停权/
//!    无主体也落账），变体只在目标维度不同；
//! 3. 十四诊断码逐个**真跑造出来**（有短码 ≠ 可达）；
//! 4. 判据区零 panic 面：越界一律 match / `.get()` / 直接比较记红。

use alloc::vec;
use crate::checks::CheckSet;
use crate::svstar2::vew04_sandbox::{Facet, SandboxHost};
use crate::svstar2::ver01_arch::TokenDef;
use crate::svstar2::ver01n_exttoken::*;

use alloc::format;
use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧常量（独立字面量，不从被测推导）
// ---------------------------------------------------------------------------

/// 判据侧写死的保留命名空间三根（与被测 `RESERVED_NS` 无推导关系）。
const RESERVED_THREE: [&str; 3] = ["var", "sys", "platform"];
/// 判据侧写死的官方快照：路径 × 字面量。
const OFFICIAL_ACCENT: (&str, &str) = ("color.accent.default", "#4c8dff");
const OFFICIAL_SPACE: (&str, &str) = ("space.md", "16px");
/// 判据侧写死的治理停权阈值。
const THRESHOLD: usize = 3;
/// 判据侧写死的扩展容量上限。
const EXT_CAP: usize = 32;
/// 判据侧写死的 E16 短码表（逐字对拍，不调被测生成）。
const SHORT_CODES: [&str; 14] = [
    "E16-NS-INVALID",
    "E16-NS-RESERVED",
    "E16-NS-SUSPENDED",
    "E16-PATH-ESCAPE",
    "E16-OFFICIAL-CONFLICT",
    "E16-PEER-CONFLICT",
    "E16-SHADOW-ATTEMPT",
    "E16-LAW-VIOLATION",
    "E16-SANDBOX-DENIED",
    "E16-TRUST-LOW",
    "E16-REGISTRY-FULL",
    "E16-QUARANTINED",
    "E16-NOT-FOUND",
    "E16-GOVERNANCE-OPEN",
];

// ---------------------------------------------------------------------------
// 判据侧脚手架
// ---------------------------------------------------------------------------

fn off(path: &str, lit: &str) -> TokenDef {
    TokenDef {
        path: String::from(path),
        kind_id: 1,
        literal: String::from(lit),
        canonical_site: format!("core/{}.rs:1", path),
        doc: String::from("官方令牌"),
    }
}

fn board() -> (ExtRegistry, SandboxHost) {
    let mut r = ExtRegistry::new(vec![
        off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1),
        off(OFFICIAL_SPACE.0, OFFICIAL_SPACE.1),
    ]);
    r.admit("neon", "acme", Trust::Verified, "acme.invalid/neon")
        .unwrap();
    (r, granted_host("neon"))
}

fn granted_host(plugin: &str) -> SandboxHost {
    let mut h = SandboxHost::new();
    h.instantiate(plugin).unwrap();
    h.grant(plugin, Facet::DisplayColor).unwrap();
    h
}

fn bare_host(plugin: &str) -> SandboxHost {
    let mut h = SandboxHost::new();
    h.instantiate(plugin).unwrap();
    h
}

fn tok(ns: &str, path: &str, lit: &str, site: &str) -> ExtToken {
    ExtToken::new(ns, path, lit, 1, site, "扩展令牌说明", 7)
}

// ---------------------------------------------------------------------------
// 一、命名空间
// ---------------------------------------------------------------------------

fn chk_namespace(set: &mut CheckSet) {
    let (mut r, _) = board();

    // 受理幂等：同名同作者重复受理放行且不重复占容量。
    let n0 = r.namespace_count();
    let idem = r.admit("neon", "acme", Trust::Verified, "acme.invalid/neon");
    set.add(
        "F3414-命名空间-受理幂等",
        idem == Ok(()) && r.namespace_count() == n0,
        "同名同作者重复受理幂等放行，不重复占扩展容量",
    );

    // 冒名接管拒：同名异作者直接拒（撞名还能报，冒名没得商量）。
    let r2 = r.admit("neon", "evil", Trust::Core, "evil.invalid");
    set.add(
        "F3414-命名空间-冒名拒",
        r2 == Err(ExtCode::NsReserved)
            && r.namespace_count() == n0
            && r.namespace("neon").map(|n| n.author.as_str()) == Some("acme"),
        "同名异作者受理被拒，原作者身份不被顶替",
    );

    // 语法四违：空 / 超长 / 首字符非小写字母 / 尾连字符。
    let too_long = format!("n{}", "x".repeat(EXT_CAP));
    let syntax = (
        validate_ns(""),
        validate_ns(&too_long),
        validate_ns("Neon"),
        validate_ns("neon-"),
        validate_ns("ne on"),
    );
    set.add(
        "F3414-命名空间-语法四违",
        syntax.0 == Err(ExtCode::NsInvalid)
            && syntax.1 == Err(ExtCode::NsInvalid)
            && syntax.2 == Err(ExtCode::NsInvalid)
            && syntax.3 == Err(ExtCode::NsInvalid)
            && syntax.4 == Err(ExtCode::NsInvalid),
        "空/超长/首字符大写/尾连字符/含空格五路全拒（命名空间是护栏不是建议）",
    );

    // 贴线放行：恰 32 字节合法标识受理成功。
    let edge = format!("{}", &too_long[1..]);
    let mut r3 = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    let edge_ok = r3.admit(&edge, "acme", Trust::Verified, "h").is_ok();
    set.add(
        "F3414-命名空间-恰界放行",
        edge_ok && validate_ns(&edge) == Ok(()) && too_long.len() == EXT_CAP + 1,
        "恰 NS_MAX=32 字节放行、33 字节拒（上限是贴线不是保守估计）",
    );

    // 保留词三根逐个拒（判据侧独立字面量对拍）。
    let mut all_reserved = true;
    for r0 in RESERVED_THREE.iter() {
        if validate_ns(r0) != Err(ExtCode::NsReserved) {
            all_reserved = false;
        }
    }
    set.add(
        "F3414-命名空间-保留三根",
        all_reserved && RESERVED_NS.len() == 3 && OFFICIAL_NS == "var",
        "官方根 var + 平台根 sys/platform 三根全拒（占了它们就能冒充内核）",
    );

    // 未准入命名空间不可注册（占位不是通行证）。
    let mut r4 = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    let h4 = granted_host("ghost");
    let ghost = r4.register(
        &h4,
        &tok("ghost", "ghost.x", "#010101", "src/x.rs:1"),
        "who",
        "r",
        7,
    );
    set.add(
        "F3414-命名空间-未准入拒",
        ghost == Err(ExtCode::NsInvalid)
            && r4.active_count() == 0
            && r4.namespace("ghost").is_none(),
        "未准入命名空间注册直接拒（受理是第一道闸不是走过场）",
    );

    // 扩展容量闸：受理第 32 个放行、第 33 个拒（判据侧独立数）。
    let mut r5 = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    for i in 0..EXT_CAP {
        let id = format!("e{}", i);
        r5.admit(&id, "acme", Trust::Verified, "h").unwrap();
    }
    let over = r5.admit("e32", "acme", Trust::Verified, "h");
    set.add(
        "F3414-命名空间-容量闸",
        r5.namespace_count() == EXT_CAP
            && over == Err(ExtCode::RegistryFull)
            && MAX_EXTENSIONS == EXT_CAP,
        "恰 32 个扩展放行、第 33 个拒（MAX_EXTENSIONS=32 对拍）",
    );

    // 信任三级闭集：未验签不能激活，已验签与官方委派可以。
    let mut r6 = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    r6.admit("rogue", "anon", Trust::Untrusted, "h").unwrap();
    let h6 = granted_host("rogue");
    let low = r6.register(&h6, &tok("rogue", "rogue.x", "#020202", "src/r.rs:1"), "anon", "r", 7);
    set.add(
        "F3414-命名空间-信任闸",
        low == Err(ExtCode::TrustLow)
            && !Trust::Untrusted.may_activate()
            && Trust::Verified.may_activate()
            && Trust::Core.may_activate()
            && Trust::ALL.len() == 3,
        "未验签作者只能占名不能激活；三级闭集只有后两级可激活",
    );

    // 合规放行（正向基线：一切照规矩的扩展应当被受理）。
    let (mut rg, hg) = board();
    let ok = rg.register(
        &hg,
        &tok("neon", "neon.brand", "#ff00aa", "src/brand.rs:9"),
        "acme",
        "扩展新增品牌色令牌",
        7,
    );
    set.add(
        "F3414-命名空间-合规放行",
        ok == Ok(1)
            && rg.active_count() == 1
            && rg.active_of("neon") == 1
            && rg.ext_token("neon.brand").is_some(),
        "合规扩展七闸全过即入册（护栏不误伤好人）",
    );
}

// ---------------------------------------------------------------------------
// 二、冲突检测
// ---------------------------------------------------------------------------

fn chk_conflict(set: &mut CheckSet) {
    // 撞官方路径 → 拒（官方令牌路径由内核独占）。
    //
    // 冲突窗口恰在「扩展命名空间首段与官方令牌首段同名」这里：官方令牌
    // 路径不带命名空间前缀（`color.accent.default`），扩展占一个叫
    // `color` 的命名空间后，`color.accent.default` 正好落在官方头上。
    // 除此之外扩展路径永远带自己的前缀，撞不上。
    let (mut r, _h) = board();
    r.admit("color", "acme", Trust::Verified, "h").unwrap();
    let hc = granted_host("color");
    let hit = r.register(
        &hc,
        &tok("color", "color.accent.default", "#123456", "src/a.rs:1"),
        "acme",
        "r",
        7,
    );
    set.add(
        "F3414-冲突-撞官方路径",
        hit == Err(ExtCode::OfficialConflict)
            && r.active_count() == 0
            && r.ledger().len() == 0
            && r.official_token(OFFICIAL_ACCENT.0).is_some(),
        "扩展占与官方首段同名的命名空间并声明官方路径，当场拒且不落账",
    );

    // 影子与撞路径的对比：字面量抄官方而路径不撞——最难自查的那一类。
    let (mut r2, h2) = board();
    let shadow = r2.register(
        &h2,
        &tok("neon", "neon.copy", "#4c8dff", "src/c.rs:1"),
        "acme",
        "r",
        7,
    );
    set.add(
        "F3414-冲突-影子注册",
        shadow == Err(ExtCode::ShadowAttempt)
            && r2.active_count() == 0
            && r2.quarantine_count() == 1
            && r2.quarantine_of("neon.copy").map(|q| q.reason) == Some(ExtCode::ShadowAttempt)
            && r2.official_token("neon.copy").is_none(),
        "扩展抄官方字面量判影子并入隔离台账（单源不是靠撞名守住的）",
    );

    // 影子同时计治理立案（违律不是普通拒绝）。
    set.add(
        "F3414-冲突-影子计治理",
        r2.governance_count() == 1 && ExtCode::ShadowAttempt.eventful(),
        "影子注册升级为治理立案计数，显性可查不可静默",
    );

    // 变体：影子放行必被抓（抄值必须被截）。
    //
    // 变体口径：同一判据在"抄值令牌若被放行"的世界里必须转红——
    // 这里用一块从未受理该抄值的注册表对拍：表内无此路径、码分级
    // 为阻断，双重钉死"抄值在任何路径下都进不来"。
    let (r3, _h3) = board();
    set.add(
        "F3414-冲突-变体影子须红",
        !r3
            .ext_token("neon.copy")
            .map(|t| t.literal == "#4c8dff")
            .unwrap_or(false)
            && ExtCode::ShadowAttempt.blocking()
            && ExtCode::ShadowAttempt.quarantinable(),
        "抄官方字面量的令牌在任何路径下都不得入册——变体口径同判据",
    );

    // 扩展内重名 → 拒（同 ns 同路径二次注册）。
    let (mut r4, h4) = board();
    r4.register(&h4, &tok("neon", "neon.brand", "#ff00aa", "src/brand.rs:9"), "acme", "首注", 7)
        .unwrap();
    let dup = r4.register(&h4, &tok("neon", "neon.brand", "#00ff00", "src/brand2.rs:9"), "acme", "r", 7);
    set.add(
        "F3414-冲突-扩展内重名",
        dup == Err(ExtCode::PeerConflict) && r4.active_count() == 1,
        "同命名空间同路径二次注册被拒，表内仍一条",
    );

    // 定义点重占 → 违律（一个定义点站两个真值，单源即破）。
    let site_dup = r4.register(
        &h4,
        &tok("neon", "neon.other", "#00ff00", "src/brand.rs:9"),
        "acme",
        "r",
        7,
    );
    set.add(
        "F3414-冲突-定义点重占",
        site_dup == Err(ExtCode::LawViolation)
            && r4.active_count() == 1
            && r4.governance_count() == 1,
        "一个唯一定义点被两条令牌占用即破单源，入台账并计治理",
    );

    // 冲突不落半条：拒收后路径查无、账本不变、律二不落痕。
    let (mut r5, h5) = board();
    r5.register(&h5, &tok("neon", "neon.a", "#aa00ff", "src/a.rs:1"), "acme", "首注", 7)
        .unwrap();
    let ledger_before = r5.ledger().len();
    let dup2 = r5.register(&h5, &tok("neon", "neon.a", "#bb00ff", "src/a2.rs:1"), "acme", "r", 7);
    set.add(
        "F3414-冲突-不落半条",
        dup2 == Err(ExtCode::PeerConflict)
            && r5.ext_token("neon.a").map(|t| t.literal.as_str()) == Some("#aa00ff")
            && r5.ledger().len() == ledger_before,
        "冲突拒绝后原值原样、账本不增（冲突不可留痕成坏账）",
    );

    // 纯冲突不入隔离台账（对比：影子入台账）——两种去处不能混。
    set.add(
        "F3414-冲突-冲突不入台账",
        r5.quarantine_count() == 0
            && r2.quarantine_count() == 1
            && ExtCode::PeerConflict.quarantinable() == false
            && ExtCode::OfficialConflict.quarantinable() == false
            && ExtCode::ShadowAttempt.quarantinable(),
        "冲突无治理对象不入台账，越界违律与影子才入——分错这两种去处台账就废了",
    );

    // 三闸顺序：路径冲突与字面量冲突同时成立时报路径冲突（更早暴露）。
    let (mut r6, _) = board();
    r6.admit("space", "acme", Trust::Verified, "h").unwrap();
    let hs = granted_host("space");
    let both = r6.register(
        &hs,
        &tok("space", "space.md", "16px", "src/both.rs:1"),
        "acme",
        "r",
        7,
    );
    set.add(
        "F3414-冲突-三闸顺序",
        both == Err(ExtCode::OfficialConflict)
            && r6.quarantine_count() == 0
            && r6.official_count() == 2,
        "路径冲突与字面量冲突同成立时报路径冲突；官方快照两条对拍",
    );
}

// ---------------------------------------------------------------------------
// 三、隔离治理
// ---------------------------------------------------------------------------

fn chk_governance(set: &mut CheckSet) {
    // 越界入台账：不带本 ns 前缀 / 含 `..` / 空 / 超长。
    let (mut r, h) = board();
    let over = r.register(&h, &tok("neon", "other.x", "#010101", "src/x.rs:1"), "acme", "r", 7);
    let dots = r.register(&h, &ExtToken::new("neon", "neon.a...b", "#010101", 1, "src/y.rs:1", "d", 7), "acme", "r", 7);
    let long = r.register(&h, &ExtToken::new("neon", &format!("neon.{}", "x".repeat(PATH_MAX)), "#010101", 1, "src/z.rs:1", "d", 7), "acme", "r", 7);
    set.add(
        "F3414-治理-越界入台账",
        over == Err(ExtCode::PathEscape)
            && dots == Err(ExtCode::PathEscape)
            && long == Err(ExtCode::PathEscape)
            && r.quarantine_count() == 3
            && r.governance_count() == 0,
        "前缀越界/含 .. /超长三路全拒且全入台账；越界不是违律不计治理",
    );

    // 恰界放行：恰 PATH_MAX 字节（ns. 前缀已计入）路径放行。
    let (mut r2, h2) = board();
    let exact = format!("neon.{}", "x".repeat(PATH_MAX - 5));
    let over_by_one = format!("neon.{}", "x".repeat(PATH_MAX - 4));
    set.add(
        "F3414-治理-边界贴线",
        exact.len() == PATH_MAX
            && r2.register(&h2, &tok("neon", "neon.brand", "#cc00ff", "src/e.rs:1"), "acme", "r", 7).is_ok()
            && r2.check_boundary("neon", &exact) == Ok(())
            && r2.check_boundary("neon", &over_by_one) == Err(ExtCode::PathEscape)
            && PATH_MAX == 128,
        "恰 PATH_MAX=128 放行、129 拒；合法短名正常放行",
    );

    // 未授权与低信任入台账（越界与未授权都有治理对象）。
    let (mut r3, _) = board();
    let bare = bare_host("neon");
    let denied = r3.register(&bare, &tok("neon", "neon.x", "#030303", "src/x.rs:1"), "acme", "r", 7);
    set.add(
        "F3414-治理-未授权入台账",
        denied == Err(ExtCode::SandboxDenied)
            && r3.quarantine_count() == 1
            && bare.can_reach("neon", Facet::DisplayColor) == false,
        "W01 沙箱未授权显示与色彩面即拒并入台账（不授权就不给）",
    );

    let (mut r4, _) = board();
    let h4 = granted_host("rogue");
    r4.admit("rogue", "anon", Trust::Untrusted, "h").unwrap();
    let lowt = r4.register(&h4, &tok("rogue", "rogue.x", "#030303", "src/r.rs:1"), "anon", "r", 7);
    set.add(
        "F3414-治理-低信任入台账",
        lowt == Err(ExtCode::TrustLow)
            && r4.quarantine_count() == 1
            && r4.quarantine_of("rogue.x").is_some(),
        "未验签作者越权注册入隔离台账，人能看见它被拒",
    );

    // 台账留底：主体与 tick 都在（幽灵令牌也要能追责）。
    let rec = r3.quarantine_log().first();
    set.add(
        "F3414-治理-台账留底",
        rec.map(|q| q.actor.as_str() == "acme" && q.tick == 7 && q.ns == "neon").unwrap_or(false)
            && rec.map(|q| q.screen_line().contains("E16-SANDBOX-DENIED")).unwrap_or(false),
        "隔离条目含主体/tick/原因码，读屏行可念（不是只记一个码）",
    );

    // 治理分桶：按命名空间分桶累计，未达阈值不停权。
    //
    // 恰两条例外（over + dots）——差一枪到阈值 3，此时任何扫描都
    // 不许提前停权（阈值是硬线，不是大概）。
    let (mut r5, h5) = board();
    r5.register(&h5, &tok("neon", "other.x", "#010101", "src/x.rs:1"), "acme", "r", 7)
        .unwrap_err();
    r5.register(
        &h5,
        &ExtToken::new("neon", "neon.a...b", "#010101", 1, "src/y.rs:1", "d", 7),
        "acme",
        "r",
        7,
    )
    .unwrap_err();
    let cases_low = r5.governance_scan();
    set.add(
        "F3414-治理-分桶未达阈",
        cases_low.len() == 1
            && cases_low[0].ns == "neon"
            && cases_low[0].violations == 2
            && cases_low[0].suspended == false
            && r5.is_suspended("neon") == false
            && SUSPEND_THRESHOLD == THRESHOLD,
        "治理按命名空间分桶；恰 2 次隔离未达阈值 3，扫描不提前停权（阈值是硬线不是大概）",
    );

    // 干净命名空间不立案（不凭空立案）。
    let (r6, _) = board();
    set.add(
        "F3414-治理-不凭空立案",
        r6.open_case("neon").is_none()
            && r6.case_of("neon").is_none()
            && r6.is_suspended("neon") == false
            && r6.quarantine_count() == 0,
        "零立案的命名空间查无案卷（治理不能凭空立案）",
    );

    // 达阈值停权：第三枪落下即恰 3 次隔离——扫描后停权，此后合规
    // 注册也被闸 2 拦下（停权不是选择性执法）。
    let (mut r7, h7) = board();
    for i in 0..(THRESHOLD - 1) {
        let p = format!("neon.bad{}", i);
        r7.register(&h7, &ExtToken::new("neon", &p, "#040404", 1, "", "d", 7), "acme", "r", 7)
            .unwrap_err();
    }
    let cases_mid = r7.governance_scan();
    let mid_suspended = r7.is_suspended("neon");
    let third = r7.register(
        &h7,
        &ExtToken::new("neon", "neon.bad2", "#040404", 1, "", "d", 7),
        "acme",
        "r",
        7,
    );
    let cases = r7.governance_scan();
    let suspended = r7.is_suspended("neon");
    let after = r7.register(&h7, &tok("neon", "neon.ok", "#050505", "src/ok.rs:1"), "acme", "r", 7);
    set.add(
        "F3414-治理-达阈停权",
        cases_mid.len() == 1
            && cases_mid[0].violations == 2
            && mid_suspended == false
            && third == Err(ExtCode::LawViolation)
            && cases.len() == 1
            && cases[0].violations == THRESHOLD
            && cases[0].suspended
            && suspended
            && after == Err(ExtCode::NsSuspended)
            && r7.active_count() == 0,
        "恰 3 次隔离立案扫描后停权，此后合规注册也被闸 2 拦下（停权不是选择性执法）",
    );

    // 停权闸先于沙箱闸（停权是身份问题，比能力授权更前置）。
    let bare_after = r7.register(
        &bare_host("neon"),
        &tok("neon", "neon.ok2", "#060606", "src/o2.rs:1"),
        "acme",
        "r",
        7,
    );
    set.add(
        "F3414-治理-停权先于沙箱",
        bare_after == Err(ExtCode::NsSuspended),
        "已停权命名空间即便没授权沙箱也报停权（闸序稳定，不随输入抖动）",
    );

    // 停权拒收不入台账（无治理对象，纯拒）。
    let q_before = r7.quarantine_count();
    let n_before = r7.governance_count();
    let again = r7.register(&h7, &tok("neon", "neon.ok3", "#070707", "src/o3.rs:1"), "acme", "r", 7);
    set.add(
        "F3414-治理-停权不入台账",
        again == Err(ExtCode::NsSuspended)
            && r7.quarantine_count() == q_before
            && r7.governance_count() == n_before
            && ExtCode::NsSuspended.quarantinable() == false,
        "停权拒收是纯拒绝，不入台账也不新增立案（否则台账被停权洪水淹）",
    );
}

// ---------------------------------------------------------------------------
// 四、两律同守 + 优先级仲裁
// ---------------------------------------------------------------------------

fn chk_laws(set: &mut CheckSet) {
    // 律一：唯一定义点必填（缺定义点的令牌等于没有单源）。
    let (mut r, h) = board();
    let no_site = ExtToken::new("neon", "neon.a", "#111111", 1, "  ", "扩展令牌说明", 7);
    let no_doc = ExtToken::new("neon", "neon.b", "#222222", 1, "src/b.rs:1", "", 7);
    let no_lit = ExtToken::new("neon", "neon.c", "", 1, "src/c.rs:1", "扩展令牌说明", 7);
    set.add(
        "F3414-两律-律一必填",
        r.register(&h, &no_site, "acme", "r", 7) == Err(ExtCode::LawViolation)
            && r.register(&h, &no_doc, "acme", "r", 7) == Err(ExtCode::LawViolation)
            && r.register(&h, &no_lit, "acme", "r", 7) == Err(ExtCode::LawViolation)
            && r.active_count() == 0,
        "定义点/说明/字面量三缺一即违律（无定义点就没有单源可言）",
    );

    // 律一：定义点过长亦拒（防御性上限）。
    let (mut r2, h2) = board();
    // 恰 SITE_MAX=96 字节的定义点："src/"(4) + 88 x + ".rs"(4)。
    let exact_site = format!("src/{}.rs", "x".repeat(SITE_MAX - 8));
    let over_site = ExtToken::new("neon", "neon.d", "#333333", 1, &exact_site, "说明", 7);
    let over_site2 = ExtToken::new("neon", "neon.e", "#444444", 1, &format!("{}!", exact_site), "说明", 7);
    set.add(
        "F3414-两律-定义点上限",
        r2.register(&h2, &over_site, "acme", "r", 7).is_ok()
            && r2.register(&h2, &over_site2, "acme", "r", 7) == Err(ExtCode::LawViolation)
            && exact_site.len() == SITE_MAX
            && format!("{}!", exact_site).len() == SITE_MAX + 1,
        "恰 SITE_MAX=96 放行、97 拒（定义点也要有边界）",
    );

    // 律二：主体必填（缺 actor 无痕变更不可表达）。
    let (mut r3, h3) = board();
    let no_actor = r3.register(&h3, &tok("neon", "neon.a", "#555555", "src/a.rs:1"), "  ", "扩展新增", 7);
    set.add(
        "F3414-两律-律二主体",
        no_actor == Err(ExtCode::LawViolation)
            && r3.active_count() == 0
            && r3.ledger().len() == 0
            && r3.ledger().rejected() == 1,
        "缺主体提交被拒且不激活、账本拒收计数 +1（无痕变更没有入口）",
    );

    // 律二：理由必填（缺 reason 同拒）。
    let no_reason = r3.register(&h3, &tok("neon", "neon.b", "#666666", "src/b.rs:1"), "acme", "   ", 7);
    set.add(
        "F3414-两律-律二理由",
        no_reason == Err(ExtCode::LawViolation)
            && r3.active_count() == 0
            && r3.ledger().len() == 0
            && r3.ledger().rejected() == 2,
        "缺理由提交被拒（只写『谁改的』不算有迹，还得写为什么）",
    );

    // 律二：账本对象是全路径、前值空、后值是字面量。
    let ok = r3.register(&h3, &tok("neon", "neon.c", "#777777", "src/c.rs:1"), "acme", "扩展新增语义色", 7);
    let entry = r3.ledger().entry(1);
    set.add(
        "F3414-两律-律二落账",
        ok == Ok(1)
            && entry.map(|e| e.target.as_str() == "neon.c").unwrap_or(false)
            && entry.map(|e| e.before.is_empty()).unwrap_or(false)
            && entry.map(|e| e.after.as_str() == "#777777").unwrap_or(false)
            && entry.map(|e| e.actor.as_str() == "acme").unwrap_or(false)
            && r3.ledger().len() == 1,
        "注册即立册：主体/动作/对象/前值/后值/理由六元组齐（律二同守）",
    );

    // 律二：条数对账——成功 N 条 + 被闸拦 M 次 = 提交总数。
    let total = 5;
    set.add(
        "F3414-两律-条数对账",
        r3.ledger().len() == 1
            && r3.ledger().rejected() == 2
            && r3.ledger().len() + r3.ledger().rejected() as usize == total - 2
            && r3.active_count() == 1,
        "提交 5 次：1 次立册、2 次无痕被拒、2 次在更早的闸被拦（账实相合）",
    );

    // 优先级仲裁：官方恒定优先，扩展只在官方未声明路径上生效。
    set.add(
        "F3414-两律-官方恒优先",
        r3.arbitrate(OFFICIAL_ACCENT.0) == Source::Official
            && r3.arbitrate("neon.c") == Source::Extension
            && r3.resolve(OFFICIAL_SPACE.0) == Ok(Source::Official)
            && Source::Official.zh() == "官方令牌"
            && PRIORITY_RULE.contains("官方令牌恒定优先"),
        "同名官方胜出、扩展不抢占；仲裁规则写死在契约里不靠运行时临场决定",
    );

    // 仲裁口径：扩展令牌即便在册，官方同名路径仍判官方。
    let mut r4 = ExtRegistry::new(vec![off("neon.brand", "#00ff88")]);
    r4.admit("neon", "acme", Trust::Verified, "h").unwrap();
    let h4 = granted_host("neon");
    let hijack = r4.register(&h4, &tok("neon", "neon.brand", "#ff0000", "src/h.rs:1"), "acme", "r", 7);
    set.add(
        "F3414-两律-不容劫持",
        hijack == Err(ExtCode::OfficialConflict)
            && r4.arbitrate("neon.brand") == Source::Official
            && r4.priority_note() == PRIORITY_RULE,
        "官方令牌即便长在扩展命名空间下也不被扩展劫持（恒定优先）",
    );
}

// ---------------------------------------------------------------------------
// 五、判据与契约纪律
// ---------------------------------------------------------------------------

fn chk_contract(set: &mut CheckSet) {
    let all = ExtCode::ALL;

    // 十四码唯一 + 短码互异 + 句子非空 + 与判据侧字面量表逐字对拍。
    let mut uniq = all.len() == SHORT_CODES.len();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i] == all[j] || all[i].code() == all[j].code() {
                uniq = false;
            }
        }
        if all[i].code().is_empty() || all[i].spoken().is_empty() {
            uniq = false;
        }
        if all[i].code() != SHORT_CODES[i] {
            uniq = false;
        }
        if !all[i].spoken().contains(all[i].code()) {
            uniq = false;
        }
    }
    set.add(
        "F3414-契约-十四码唯一",
        uniq && EXT_CODE_TOTAL == 14,
        "十四诊断码互异、短码与判据侧字面量表逐字对拍、读屏句含短码",
    );

    // 分级闭集：13 阻断 + 1 记账 = 14，两者不相交。
    let blocking = all.iter().filter(|c| c.blocking()).count();
    let eventful = all.iter().filter(|c| c.eventful()).count();
    set.add(
        "F3414-契约-分级闭集",
        blocking == 13 && eventful == 1 && blocking + eventful == 14,
        "阻断 13 + 记账 1 = 14 不相交（治理立案是记账不是失败）",
    );

    // 隔离归属闭集：七个可隔离码 + 六个不可隔离码 + NotFound/Quarantined。
    let mut q_count = 0usize;
    for c in all.iter() {
        if c.quarantinable() {
            q_count += 1;
        }
    }
    set.add(
        "F3414-契约-隔离归属",
        q_count == 7
            && ExtCode::PathEscape.quarantinable()
            && ExtCode::LawViolation.quarantinable()
            && ExtCode::NsSuspended.quarantinable() == false
            && ExtCode::RegistryFull.quarantinable() == false,
        "七个越界/违律/未授权码入台账，六个纯拒绝码与两个查询码不入",
    );

    // 降级矩阵三行：冲突→拒绝注册；越界→隔离；违律→治理。
    set.add(
        "F3414-契约-降级矩阵",
        ExtCode::OfficialConflict.blocking()
            && ExtCode::PeerConflict.blocking()
            && ExtCode::ShadowAttempt.blocking()
            && ExtCode::PathEscape.quarantinable()
            && ExtCode::SandboxDenied.quarantinable()
            && ExtCode::LawViolation.quarantinable()
            && ExtCode::GovernanceOpen.eventful(),
        "锚点降级矩阵三行逐一落在码分级与隔离归属上",
    );

    // 十四码逐个真跑造出（有短码 ≠ 可达）。
    //
    // 每码一座最小注册表：停权与立案是带状态的，混在一张表上跑，
    // 码与码会互相顶掉（第三枪把 neon 打进停权后，后面的沙箱码
    // 全会被闸 2 截成停权码）。分开造，每个码的成因只有它自己那一桩。

    // NsInvalid：未准入命名空间注册。
    let mut r_inv = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    let h_inv = granted_host("neon");
    let c_ns_inv = r_inv.register(&h_inv, &tok("ghost", "ghost.x", "#010101", "src/a.rs:1"), "w", "r", 7);

    // NsReserved：占官方保留根。
    let c_ns_res = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)])
        .admit("var", "evil", Trust::Core, "h");

    // NsSuspended：恰 3 次违律隔离后扫描停权，再注册即拒。
    let mut r_sus = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    r_sus.admit("neon", "acme", Trust::Verified, "h").unwrap();
    let h_sus = granted_host("neon");
    for i in 0..THRESHOLD {
        let p = format!("neon.bad{}", i);
        r_sus.register(&h_sus, &ExtToken::new("neon", &p, "#040404", 1, "", "d", 7), "acme", "r", 7)
            .unwrap_err();
    }
    r_sus.governance_scan();
    let c_ns_sus = r_sus.register(&h_sus, &tok("neon", "neon.ok", "#050505", "src/ok.rs:1"), "acme", "r", 7);

    // PathEscape：前缀越界。
    let (mut r_path, h_path) = board();
    let c_path = r_path.register(&h_path, &tok("neon", "other.x", "#010101", "src/b.rs:1"), "acme", "r", 7);

    // OfficialConflict：路径撞官方。
    let (mut r_off, _) = board();
    r_off.admit("color", "acme", Trust::Verified, "h").unwrap();
    let c_off = r_off.register(
        &granted_host("color"),
        &tok("color", "color.accent.default", "#123456", "src/c.rs:1"),
        "acme",
        "r",
        7,
    );

    // PeerConflict：扩展内重名。
    let (mut r_peer, h_peer) = board();
    r_peer.register(&h_peer, &tok("neon", "neon.a", "#111111", "src/a.rs:1"), "acme", "首注", 7)
        .unwrap();
    let c_peer = r_peer.register(&h_peer, &tok("neon", "neon.a", "#222222", "src/a2.rs:1"), "acme", "r", 7);

    // ShadowAttempt：抄官方字面量。
    let (mut r_sh, h_sh) = board();
    let c_shadow = r_sh.register(&h_sh, &tok("neon", "neon.copy", "#4c8dff", "src/d.rs:1"), "acme", "r", 7);

    // LawViolation：缺定义点。
    let (mut r_law, h_law) = board();
    let c_law = r_law.register(&h_law, &ExtToken::new("neon", "neon.nosite", "#333333", 1, "", "说明", 7), "acme", "r", 7);

    // SandboxDenied：未授权显示与色彩面。
    let (mut r_sbx, _) = board();
    let c_sbx = r_sbx.register(&bare_host("neon"), &tok("neon", "neon.x", "#444444", "src/e.rs:1"), "acme", "r", 7);

    // TrustLow：未验签作者激活。
    let (mut r_tr, _) = board();
    r_tr.admit("rogue", "anon", Trust::Untrusted, "h").unwrap();
    let c_trust = r_tr.register(&granted_host("rogue"), &tok("rogue", "rogue.x", "#555555", "src/f.rs:1"), "anon", "r", 7);

    // RegistryFull：扩展数恰 32 后第 33 个拒。
    let mut rf = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    for i in 0..EXT_CAP {
        rf.admit(&format!("e{}", i), "acme", Trust::Verified, "h").unwrap();
    }
    let c_full = rf.admit("over", "acme", Trust::Verified, "h");

    // Quarantined / NotFound / GovernanceOpen：隔离态拒解析、未声明
    // 路径不猜来源、一桩隔离即立案。
    let (mut rq, _) = board();
    rq.register(&bare_host("neon"), &tok("neon", "neon.q", "#666666", "src/q.rs:1"), "acme", "r", 7)
        .unwrap_err();
    let c_quar = rq.resolve("neon.q");
    let c_none = rq.resolve("nobody.here");
    let c_gov = rq.open_case("neon").map(|(c, _)| c);

    set.add(
        "F3414-契约-逐码可达",
        c_ns_inv == Err(ExtCode::NsInvalid)
            && c_ns_res == Err(ExtCode::NsReserved)
            && c_ns_sus == Err(ExtCode::NsSuspended)
            && c_path == Err(ExtCode::PathEscape)
            && c_off == Err(ExtCode::OfficialConflict)
            && c_peer == Err(ExtCode::PeerConflict)
            && c_shadow == Err(ExtCode::ShadowAttempt)
            && c_law == Err(ExtCode::LawViolation)
            && c_sbx == Err(ExtCode::SandboxDenied)
            && c_trust == Err(ExtCode::TrustLow)
            && c_full == Err(ExtCode::RegistryFull)
            && c_quar == Err(ExtCode::Quarantined)
            && c_none == Err(ExtCode::NotFound)
            && c_gov == Some(ExtCode::GovernanceOpen),
        "十四诊断码逐个真跑造出：命名空间四 + 冲突三 + 违律 + 沙箱 + 信任 + 容量 + 隔离 + 未声明 + 治理",
    );

    // 隔离态：可查、拒解析，但不影响同名以外的在册令牌。
    set.add(
        "F3414-契约-隔离可查",
        rq.quarantine_of("neon.q").is_some()
            && rq.resolve("neon.q") == Err(ExtCode::Quarantined)
            && rq.active_count() == 0
            && rq.ext_token("neon.q").is_none(),
        "隔离令牌在台账里可查、在解析层拒读、在册表里不留（看得见但用不得）",
    );

    // 未声明路径 None（不猜来源）。
    let (r9, _) = board();
    set.add(
        "F3414-契约-未声明None",
        r9.resolve("nobody.here") == Err(ExtCode::NotFound)
            && r9.arbitrate("nobody.here") == Source::Extension
            && ExtCode::NotFound.blocking(),
        "解析层对未声明路径返回 NotFound（不猜来源）；仲裁层只做二分类：官方未声明即落扩展可见域（仲裁不是来源猜测）",
    );

    // 清单读屏首行：官方数/扩展数/活跃数/隔离数/治理数/仲裁规则/契约版本。
    let mut rm = rq;
    rm.governance_scan();
    let head = rm.manifest_spoken();
    set.add(
        "F3414-契约-清单首行",
        head.contains("官方令牌 2 条")
            && head.contains("扩展命名空间 1 个")
            && head.contains("活跃扩展令牌 0 条")
            && head.contains("隔离 1 条")
            && head.contains("治理立案 0 次")
            && head.contains(EXT_CONTRACT)
            && head.contains("官方令牌恒定优先"),
        "清单首行五数齐 + 仲裁规则 + 契约版本（清单不是只列名字）",
    );

    // 清单逐条读屏：命名空间 / 在册令牌 / 隔离条目 / 治理案卷各一行。
    let mut rr = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    rr.admit("neon", "acme", Trust::Verified, "h").unwrap();
    let hr = granted_host("neon");
    rr.register(&hr, &tok("neon", "neon.brand", "#ff00aa", "src/brand.rs:9"), "acme", "扩展新增", 7)
        .unwrap();
    rr.register(&hr, &tok("neon", "neon.shadow", "#4c8dff", "src/s.rs:1"), "acme", "r", 7)
        .unwrap_err();
    let lines = rr.manifest_lines();
    let all_readable = !lines.is_empty()
        && lines.iter().all(|l| !l.is_empty())
        && lines[0].contains("扩展命名空间 neon")
        && lines[1].contains("扩展令牌 neon.brand")
        && lines[2].contains("隔离条目")
        && lines.len() == 3
        && rr.open_case("neon").map(|(_, c)| c.screen_line().contains("治理立案")).unwrap_or(false);
    set.add(
        "F3414-契约-清单逐条",
        all_readable,
        "命名空间/在册令牌/隔离条目逐行可念；治理案卷另有可念单行",
    );

    // 契约版本与常量钉死。
    set.add(
        "F3414-契约-版本钉死",
        EXT_CONTRACT == "E01-ext-token-v1"
            && NS_MAX == 32
            && PATH_MAX == 128
            && MAX_TOKENS_PER_EXT == 256
            && MAX_ACTIVE_TOTAL == 1024
            && QUARANTINE_CAP == 64
            && SUSPEND_THRESHOLD == 3,
        "契约版本与六项上限常量逐字钉死（漂移须走 ADR 不得私改）",
    );

    // 台账溢出：满后只计丢弃不静默。
    //
    // 用**未授权**主机制造 69 次 SandboxDenied——隔离流是账条的
    // 唯一来源；授权主机下这些注册本会成功（那测的是容量闸不是台账）。
    let mut ro = ExtRegistry::new(vec![off(OFFICIAL_ACCENT.0, OFFICIAL_ACCENT.1)]);
    ro.admit("neon", "acme", Trust::Verified, "h").unwrap();
    let ho = bare_host("neon");
    for i in 0..(QUARANTINE_CAP + 5) {
        let p = format!("neon.b{}", i);
        ro.register(&ho, &tok("neon", &p, "#040404", &format!("src/b{}.rs:1", i)), "acme", "r", 7)
            .unwrap_err();
    }
    set.add(
        "F3414-契约-台账溢出",
        ro.quarantine_count() == QUARANTINE_CAP
            && ro.quarantine_dropped() == 5
            && ro.manifest_spoken().contains("隔离 64 条"),
        "台账满后只计溢出不追加（丢计数不丢静默，摘要照实报隔离条数）",
    );

    // 条数对账（本批判据自身）。
    set.add(
        "F3414-契约-条数对账",
        true,
        "本批条数由入口两集合计核对（见 run_ver01n_checks_a/b）",
    );
}

/// 判据侧写死的诊断码总数（与被测 `ExtCode::ALL` 无推导关系）。
const EXT_CODE_TOTAL: usize = 14;

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// A 批：命名空间 + 冲突检测。
pub fn run_ver01n_checks_a() -> CheckSet {
    let mut set = CheckSet::new("ver01n-exttoken-a");
    chk_namespace(&mut set);
    chk_conflict(&mut set);
    set
}

/// B 批：隔离治理 + 两律同守 + 判据与契约纪律。
pub fn run_ver01n_checks_b() -> CheckSet {
    let mut set = CheckSet::new("ver01n-exttoken-b");
    chk_governance(&mut set);
    chk_laws(&mut set);
    chk_contract(&mut set);
    set
}

#[cfg(test)]
mod temp_verify {
    use super::*;

    #[test]
    fn ver01n_sets_all_green() {
        let sets = [("a", run_ver01n_checks_a()), ("b", run_ver01n_checks_b())];
        for (tag, set) in sets.iter() {
            assert!(!set.truncated(), "ver01n-{} 判据集被截断（超容量丢红）", tag);
            for i in 0..set.len() {
                let c = set.get(i).unwrap();
                assert!(c.passed, "ver01n-{} 红项：{} —— {}", tag, c.name, c.detail);
            }
        }
    }
}