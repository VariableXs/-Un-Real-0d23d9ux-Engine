//! VE-F5202 域自检（VE-Z 域 · 特效库定位与分类判据层）
//!
//! 判据侧**独立写死**期望值（四域桶归属、三律码、密度边界 16/17、读屏
//! 行数预算 6），不复用实现侧常量。聚合防自调：族内判据只调另一族
//! standalone + 进行中 set 自身 tally（F0817 首版自我递归栈溢出教训），
//! 守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vez01_vfxarch::{Semantic, Subsystem, VfxEntry};
use super::vez02_taxonomy as tx;
use super::vez02_taxonomy::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

/// 判据侧独立写死的四域线码段与细分码。
const EXPECT_CODES: [u16; 4] = [0x5200, 0x5201, 0x5202, 0x5203];

/// 判据侧独立写死的密度边界（节制律 16/17 恰边界）。
const EXPECT_MAX_DENSITY: usize = 16;

/// 判据侧独立写死的读屏行数预算（表头 1 + 域 4 + 尾 1）。
const EXPECT_SCREEN_ROWS: usize = 6;

/// 合法特效构造（类别字面量即 'static；语义给非空占位）。
fn entry(name: &str, class: &'static str) -> VfxEntry {
    VfxEntry {
        name: String::from(name),
        owner: Subsystem::Particle,
        class,
        semantic: Semantic::Declared(String::from("测试语义")),
    }
}

/// 合法声明卡（语义/可关/受密度约束）。
fn good_card(name: &str) -> LawCard {
    LawCard::new(name, Some("测试语义"), true, true)
}

// ---------------------------------------------------------------------------
// 一、规格（四域分类 / 三律）
// ---------------------------------------------------------------------------

fn chk_spec_four_domains(set: &mut CheckSet) {
    // 规格-01：四域分类——四类各入正确桶（判据侧独立写死归属）。
    let mut cat = DomainCatalog::new();
    let pairs: [(&str, &'static str, Domain); 4] = [
        ("重斩", "打击感", Domain::Combat),
        ("暴雨", "天气昼夜", Domain::Ambient),
        ("回忆杀", "剧情演出", Domain::Narrative),
        ("按钮涟漪", "UI动效", Domain::Interface),
    ];
    let mut all_ok = true;
    for &(name, class, expect_domain) in pairs.iter() {
        let e = entry(name, class);
        let c = good_card(name);
        match cat.classify(&e, &c) {
            Placement::Filed { domain, density_warned: false } => {
                if domain != expect_domain || !cat.contains(name) {
                    all_ok = false;
                }            }
            _ => all_ok = false,
        }
    }
    if all_ok && cat.total() == 4 {
        set.ok("EZ2-规格-01-四域分类");
    } else {
        set.fail("EZ2-规格-01-四域分类", "归属漂移或桶册未收");
    }
    // 规格-02：封闭全集双向（ordinal/of_ordinal 往返 + 越界 None）。
    let mut rt = true;
    let mut i = 0usize;
    while i < 4 {
        match Domain::of_ordinal(i) {
            Some(d) => {
                if d.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    if rt && Domain::of_ordinal(4).is_none() {
        set.ok("EZ2-规格-02-四域封闭往返");
    } else {
        set.fail("EZ2-规格-02-四域封闭往返", "往返失配或越界未拒");
    }
}

fn chk_spec_semantic_law(set: &mut CheckSet) {
    // 规格-03：语义律——三来源无语义皆拒且码专属：
    // ① Semantic::Undeclared；② 声明卡 None；③ 语义描述空白。
    let mut cat = DomainCatalog::new();
    let mut e = entry("炫技闪光", "UI动效");
    e.semantic = Semantic::Undeclared;
    let r1 = cat.classify(&e, &good_card("炫技闪光"));
    let mut cat2 = DomainCatalog::new();
    let e2 = entry("无语义雨", "天气昼夜");
    let none_card = LawCard::new("无语义雨", None, true, true);
    let r2 = cat2.classify(&e2, &none_card);
    let mut cat3 = DomainCatalog::new();
    cat3.semantics.declare("空白描述", "天气昼夜");
    let e3 = entry("空白描述", "天气昼夜");
    let blank_card = LawCard::new("空白描述", Some(""), true, true);
    let r3 = cat3.classify(&e3, &blank_card);
    let all_reject = r1 == Placement::Rejected(E_VFX2_NO_SEMANTIC)
        && r2 == Placement::Rejected(E_VFX2_NO_SEMANTIC)
        && r3 == Placement::Rejected(E_VFX2_NO_SEMANTIC)
        && cat.rejected == 1
        && cat2.rejected == 1
        && cat3.rejected == 1;
    if all_reject {
        set.ok("EZ2-规格-03-语义律三来源皆拒");
    } else {
        set.fail("EZ2-规格-03-语义律三来源皆拒", "存在无语义特效入库或码不专属");
    }
    // 规格-04：语义标注表——查无 None、空串 Some("")、覆盖更新三分可辨。
    let mut t = SemanticTable::new();
    let none_case: Option<String> = t.lookup("未登记").map(String::from);
    t.declare("空白", "");
    let blank_case: Option<String> = t.lookup("空白").map(String::from);
    t.declare("正常", "表达雨天压迫感");
    let normal_case: Option<String> = t.lookup("正常").map(String::from);
    t.declare("正常", "改为表达雨后天晴");
    let updated: Option<String> = t.lookup("正常").map(String::from);
    let three_way = none_case.is_none()
        && blank_case == Some(String::from(""))
        && normal_case == Some(String::from("表达雨天压迫感"))
        && updated == Some(String::from("改为表达雨后天晴"));
    if three_way {
        set.ok("EZ2-规格-04-语义表三分可辨");
    } else {
        set.fail("EZ2-规格-04-语义表三分可辨", "查无/空白/正常三态混淆");
    }
}

fn chk_spec_density_law(set: &mut CheckSet) {
    // 规格-05：节制律——上限内不告警、恰+1 告警（16/17 恰边界）。
    let mut cat = DomainCatalog::new();
    let mut i = 0usize;
    let mut last = Placement::Rejected(0);
    while i < EXPECT_MAX_DENSITY {
        let name = alloc::format!("战斗特效{}", i);
        let e = entry(&name, "打击感");
        last = cat.classify(&e, &good_card(&name));
        i += 1;
    }
    let quiet = cat.density_warnings == 0
        && last == Placement::Filed { domain: Domain::Combat, density_warned: false };
    let name17 = alloc::format!("战斗特效{}", EXPECT_MAX_DENSITY);
    let e17 = entry(&name17, "打击感");
    let over = cat.classify(&e17, &good_card(&name17));
    let loud = cat.density_warnings == 1
        && over == Placement::Filed { domain: Domain::Combat, density_warned: true }
        && cat.density(Domain::Combat) == EXPECT_MAX_DENSITY + 1;
    let advice = cat.convergence_advice(Domain::Combat);
    if quiet && loud && !advice.is_empty() {
        set.ok("EZ2-规格-05-节制律恰边界");
    } else {
        set.fail("EZ2-规格-05-节制律恰边界", "密度告警未在恰边界翻面或建议缺失");
    }
}

fn chk_spec_closable_law(set: &mut CheckSet) {
    // 规格-06：可关律——不可关断言拦截；主开关关停读屏如实。
    let mut cat = DomainCatalog::new();
    let e = entry("强制炫光", "UI动效");
    let locked = LawCard::new("强制炫光", Some("表达升级确认"), true, false);
    let r = cat.classify(&e, &locked);
    let blocked = r == Placement::Rejected(E_VFX2_NOT_CLOSABLE) && cat.closable_blocks == 1;
    // 主开关：关停后读屏清单首行如实。
    cat.master = MasterSwitch::Off;
    let rows = cat.screen_listing();
    let honest = rows[0].contains("已关停");
    if blocked && honest {
        set.ok("EZ2-规格-06-可关律拦截与关停");
    } else {
        set.fail("EZ2-规格-06-可关律拦截与关停", "不可关未拦或关停未如实");
    }
}

// ---------------------------------------------------------------------------
// 二、边界（上游闪烁保护 / 归属失败 / 读屏预算 / 对接面）
// ---------------------------------------------------------------------------

fn chk_bound_flicker_guard(set: &mut CheckSet) {
    // 边界-01：F4508 闪烁保护——保护开启时闪烁类不可关也被强制可关放行；
    // 保护未开启时同卡被拦（两侧翻面）。
    let e = entry("频闪爆闪", "闪烁特效");
    let card = LawCard::new("频闪爆闪", Some("表达雷击瞬间"), true, false);
    let mut off = DomainCatalog::new();
    let rejected = off.classify(&e, &card);
    let mut on = DomainCatalog::new();
    on.flicker = FlickerGuard::Active;
    let forced = on.classify(&e, &card);
    let ok = rejected == Placement::Rejected(E_VFX2_NOT_CLOSABLE)
        && matches!(forced, Placement::Filed { domain: Domain::Ambient, .. });
    if ok {
        set.ok("EZ2-边界-01-闪烁保护强制可关");
    } else {
        set.fail("EZ2-边界-01-闪烁保护强制可关", "保护未随上游状态翻面");
    }
    // 边界-02：归属失败——册外类别专属码拒绝。
    let mut cat = DomainCatalog::new();
    let weird = entry("神秘效果", "册外类别");
    let r = cat.classify(&weird, &good_card("神秘效果"));
    if r == Placement::Rejected(E_VFX2_NO_DOMAIN) && cat.rejected == 1 {
        set.ok("EZ2-边界-02-册外类别拒绝");
    } else {
        set.fail("EZ2-边界-02-册外类别拒绝", "册外类别未拒或码漂移");
    }
    // 边界-03：读屏行数预算固定 6 行（表头+4 域+尾）——判据侧写死。
    let cat = DomainCatalog::new();
    let rows = cat.screen_listing();
    if rows.len() == EXPECT_SCREEN_ROWS && rows[5].contains("密度告警") {
        set.ok("EZ2-边界-03-读屏行数预算");
    } else {
        set.fail("EZ2-边界-03-读屏行数预算", "读屏清单行数漂移或缺计数尾行");
    }
}

fn chk_bound_downstream(set: &mut CheckSet) {
    // 边界-04：下游 F5201 对接——VfxEntry 消费 + 四域总账守恒
    // （ledger 计数 = 桶长和 = total；三计数器逐位对账）。
    let mut cat = DomainCatalog::new();
    let names: [(&str, &'static str, Domain); 3] = [
        ("斩击", "打击感", Domain::Combat),
        ("暮色", "环境", Domain::Ambient),
        ("过场淡出", "剧情演出", Domain::Narrative),
    ];
    for &(name, class, _) in names.iter() {
        let e = entry(name, class);
        let _ = cat.classify(&e, &good_card(name));
    }
    let led = cat.ledger();
    let sum = led.per_domain[0] + led.per_domain[1] + led.per_domain[2] + led.per_domain[3];
    let conserved = sum == 3 && cat.total() == 3 && led.master_wire == 1;
    if conserved {
        set.ok("EZ2-边界-04-总账守恒");
    } else {
        set.fail("EZ2-边界-04-总账守恒", "分类账与桶册计数失守");
    }
    // 边界-05：F5220 双签闸前向——主开关关停后线值翻 0。
    let mut cat2 = DomainCatalog::new();
    cat2.master = MasterSwitch::Off;
    let led2 = cat2.ledger();
    if led2.master_wire == 0 {
        set.ok("EZ2-边界-05-双签闸线值翻面");
    } else {
        set.fail("EZ2-边界-05-双签闸线值翻面", "关停线值未翻面");
    }
}

fn chk_idem_determinism(set: &mut CheckSet) {
    // 幂等-01：分类确定性——同输入两次分类结局逐字段相等（对独立空册）。
    let run = || {
        let mut cat = DomainCatalog::new();
        let e = entry("暴雨", "天气昼夜");
        cat.classify(&e, &good_card("暴雨"))
    };
    if run() == run() {
        set.ok("EZ2-幂等-01-分类确定性");
    } else {
        set.fail("EZ2-幂等-01-分类确定性", "同输入不同结局");
    }
    // 幂等-02：清册重建——新册三计数器与桶全零（降级回滚面）。
    let fresh = DomainCatalog::new();
    let led = fresh.ledger();
    let zero = led.per_domain == [0, 0, 0, 0]
        && led.law_counters == [0, 0, 0]
        && fresh.total() == 0
        && fresh.semantics.is_empty();
    if zero {
        set.ok("EZ2-幂等-02-清册重建归零");
    } else {
        set.fail("EZ2-幂等-02-清册重建归零", "新册计数或桶未归零");
    }
}

// ---------------------------------------------------------------------------
// 三、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vez02_taxonomy_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EZ2-判据-01-判据面零 panic");
    } else {
        set.fail("EZ2-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-07：生产面零 panic（扫实现文件）。
    let src = include_str!("vez02_taxonomy.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EZ2-规格-07-生产面零 panic");
    } else {
        set.fail("EZ2-规格-07-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——四码皆 0x52 细分段、互异、与写死值逐位等。
    let got = [E_VFX2_NO_SEMANTIC, E_VFX2_DENSITY, E_VFX2_NOT_CLOSABLE, E_VFX2_NO_DOMAIN];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x5200 || got[i] != EXPECT_CODES[i] {
            ok = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("EZ2-判据-02-码段独占独立复核");
    } else {
        set.fail("EZ2-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 13 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vez02_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 13;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EZ2-判据-03-聚合守恒防自调");
    } else {
        set.fail("EZ2-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+幂等 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 幂等。
pub fn run_vez02_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vez02/a");
    chk_spec_four_domains(&mut s);
    chk_spec_semantic_law(&mut s);
    chk_spec_density_law(&mut s);
    chk_spec_closable_law(&mut s);
    chk_bound_flicker_guard(&mut s);
    chk_bound_downstream(&mut s);
    chk_idem_determinism(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vez02_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vez02/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vez02_checks() -> CheckSet {
    CheckSet::merge(run_vez02_checks_a_standalone(), run_vez02_checks_b_standalone())
}
