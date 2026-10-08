//! CGPU-F1601 域自检（CGPU-K 域开工判据层）
//!
//! 判据侧**独立写死**期望（七件名册、六值枚举、十组连续、五层秩、0x6B 码段）。
//! 聚合防自调：族内判据只调另一族 standalone + 进行中 set 自身 tally，守恒
//! 断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vck01_hetarch::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_CODES: [u16; 6] = [
    0x6B00, 0x6B01, 0x6B02, 0x6B03, 0x6B04, 0x6B05,
];
const EXPECT_SEVEN: usize = 7;
const EXPECT_THEMES: usize = 5;
const EXPECT_GROUPS: usize = 10;
const EXPECT_LAYERS: usize = 5;
const EXPECT_RESOURCES: usize = 6;
const EXPECT_DOMAIN_SIZE: usize = 160;

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

// ---------------------------------------------------------------------------
// 一、规格（使命 / 签收 / 枚举兑现 / 映射 / 五层）
// ---------------------------------------------------------------------------

fn chk_spec_mission(set: &mut CheckSet) {
    // 规格-01：使命可 grep——判据词「同僚」「第二曲线」独立 contains 双验。
    let m = MISSION;
    let ok = m.contains("同僚") && m.contains("第二曲线") && m.contains("统一任务图")
        && m.contains("统一预算") && m.contains("统一遥测");
    if ok {
        set.ok("EK601-规格-01-使命判据词在句");
    } else {
        set.fail("EK601-规格-01-使命判据词在句", "使命漂移或判据词缺失");
    }
    // 规格-02：七件签收全签兑付——签 7 次恰成、计数 7、ready true。
    let mut led = HandoffLedger::new();
    let mut all_ok = true;
    let mut i = 0usize;
    while i < EXPECT_SEVEN {
        if led.sign(i).is_err() {
            all_ok = false;
        }
        i += 1;
    }
    let ok = all_ok && led.sign_actions == 7 && led.signed_count() == 7 && led.ready_to_extend();
    if ok {
        set.ok("EK601-规格-02-七件全签兑付");
    } else {
        set.fail("EK601-规格-02-七件全签兑付", "全签未兑付或计数漂移");
    }
}

fn chk_spec_handoff_edges(set: &mut CheckSet) {
    // 边界-01：重复签与越界双向显性拒（签收历史不可静默改写）。
    let mut led = HandoffLedger::new();
    let first = led.sign(0);
    let dup = led.sign(0);
    let oor = led.sign(7);
    let is_signed_oor = led.is_signed(9);
    let ok = first.is_ok()
        && dup == Err(E_K601_DUP_SIGN)
        && oor == Err(E_K601_SIGN_RANGE)
        && is_signed_oor == Err(E_K601_SIGN_RANGE)
        && led.sign_actions == 1;
    if ok {
        set.ok("EK601-边界-01-重复签越界双向拒");
    } else {
        set.fail("EK601-边界-01-重复签越界双向拒", "签收闸未两侧钉死");
    }
    // 边界-02：缺件不兑付——只签六件（缺第七件）ready 必假。
    let mut led = HandoffLedger::new();
    let mut i = 0usize;
    while i < EXPECT_SEVEN - 1 {
        let _ = led.sign(i);
        i += 1;
    }
    let ok = led.signed_count() == 6 && !led.ready_to_extend();
    if ok {
        set.ok("EK601-边界-02-缺件不兑付");
    } else {
        set.fail("EK601-边界-02-缺件不兑付", "六件即兑付=签收闸被架空");
    }
}

fn chk_spec_promise_f0406(set: &mut CheckSet) {
    // 规格-03：ResourceType 六值封闭往返 + 越界 None。
    let mut rt = true;
    let mut i = 0usize;
    while i < EXPECT_RESOURCES {
        match ResourceType::of_ordinal(i) {
            Some(t) => {
                if t.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    let oor = ResourceType::of_ordinal(6).is_none();
    if rt && oor && ResourceType::ALL.len() == EXPECT_RESOURCES {
        set.ok("EK601-规格-03-六值封闭往返");
    } else {
        set.fail("EK601-规格-03-六值封闭往返", "枚举漂移或越界未拒");
    }
    // 规格-04：兑现记录独立写死对拍（预留原文/来源/兑现单/前后值数）。
    let ok = PROMISE_F0406.source == "CGPU-F0406"
        && PROMISE_F0406.fulfilled_by == "CGPU-F1601"
        && PROMISE_F0406.before_values == 2
        && PROMISE_F0406.after_values == 6
        && PROMISE_F0406.promise.contains("资源类型枚举开放");
    if ok {
        set.ok("EK601-规格-04-兑现记录对拍");
    } else {
        set.fail("EK601-规格-04-兑现记录对拍", "兑现账字段漂移");
    }
}

fn chk_spec_groups_layers(set: &mut CheckSet) {
    // 规格-05：五主题封闭 + 十组承接合法（0 仅限 K01/K10）。
    let ok = THEMES.len() == EXPECT_THEMES
        && THEMES[0] == "异构资源抽象"
        && THEMES[4] == "异构遥测"
        && GROUPS.len() == EXPECT_GROUPS
        && group_themes_valid();
    if ok {
        set.ok("EK601-规格-05-主题封闭组承接合法");
    } else {
        set.fail("EK601-规格-05-主题封闭组承接合法", "主题/组映射漂移");
    }
    // 规格-06：十组连续 + 域段 160 单守恒（判据侧独立重算 1760-1601+1）。
    let recount = 1760usize - 1601usize + 1;
    let ok = groups_contiguous() && domain_size() == EXPECT_DOMAIN_SIZE && recount == EXPECT_DOMAIN_SIZE;
    if ok {
        set.ok("EK601-规格-06-组连续域段守恒");
    } else {
        set.fail("EK601-规格-06-组连续域段守恒", "组断洞或域段失守");
    }
    // 规格-07：五层顺序秩 + 越界拒（第六层不存在）。
    let l0 = layer_of(0);
    let l4 = layer_of(4);
    let oor = layer_of(5);
    let ok = l0 == Ok("资源抽象层")
        && l4 == Ok("遥测归因层")
        && oor == Err(E_K601_LAYER_RANGE)
        && ARCH_LAYERS.len() == EXPECT_LAYERS;
    if ok {
        set.ok("EK601-规格-07-五层秩与越界拒");
    } else {
        set.fail("EK601-规格-07-五层秩与越界拒", "层序漂移或越界未拒");
    }
}

// ---------------------------------------------------------------------------
// 二、两域关系 / 不变量 / 风险
// ---------------------------------------------------------------------------

fn chk_spec_relations(set: &mut CheckSet) {
    // 规格-08：B/J 关系逐句可 grep（扩展不改核 / F1458 复用）。
    let ok = B_RELATION.contains("不改 B 内核") && B_RELATION.contains("任务图")
        && J_RELATION.contains("F1458") && J_RELATION.contains("统一账本");
    if ok {
        set.ok("EK601-规格-08-BJ关系逐句在案");
    } else {
        set.fail("EK601-规格-08-BJ关系逐句在案", "关系声明漂移");
    }
    // 规格-09：不变量双向——双真才成立，任一假即破缺（>⇄|| 变异可抓）。
    let both = invariant_contract_holds(true, true);
    let left_fail = invariant_contract_holds(false, true);
    let right_fail = invariant_contract_holds(true, false);
    let none = invariant_contract_holds(false, false);
    let ok = both && !left_fail && !right_fail && !none;
    if ok {
        set.ok("EK601-规格-09-不变量双向判定");
    } else {
        set.fail("EK601-规格-09-不变量双向判定", "合同跨引擎判定被架空");
    }
    // 规格-10：风险三条各配非空预案（风险不裸奔）。
    let mut ok = RISKS.len() == 3;
    let mut i = 0usize;
    while i < RISKS.len() {
        if RISKS[i].0.is_empty() || RISKS[i].1.is_empty() {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EK601-规格-10-风险预案齐备");
    } else {
        set.fail("EK601-规格-10-风险预案齐备", "风险缺预案");
    }
}

// ---------------------------------------------------------------------------
// 三、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vck01_hetarch_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EK601-判据-01-判据面零 panic");
    } else {
        set.fail("EK601-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-11：生产面零 panic（扫实现文件）。
    let src = include_str!("vck01_hetarch.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EK601-规格-11-生产面零 panic");
    } else {
        set.fail("EK601-规格-11-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——六码皆 0x6B 细分段、互异、与写死值逐位等。
    let got = [
        E_K601_SIGN_RANGE,
        E_K601_DUP_SIGN,
        E_K601_NOT_READY,
        E_K601_THEME_RANGE,
        E_K601_LAYER_RANGE,
        E_K601_ENGINE_RANGE,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x6B00 || got[i] != EXPECT_CODES[i] {
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
        set.ok("EK601-判据-02-码段独占独立复核");
    } else {
        set.fail("EK601-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 12 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vck01_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EK601-判据-03-聚合守恒防自调");
    } else {
        set.fail("EK601-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界。
pub fn run_vck01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vck01/a");
    chk_spec_mission(&mut s);
    chk_spec_handoff_edges(&mut s);
    chk_spec_promise_f0406(&mut s);
    chk_spec_groups_layers(&mut s);
    chk_spec_relations(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vck01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vck01/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vck01_checks() -> CheckSet {
    CheckSet::merge(run_vck01_checks_a_standalone(), run_vck01_checks_b_standalone())
}
