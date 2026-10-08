//! VE-F2807 · 域自检（判据逐条对应，见 `veo07_rules.rs` 头注）。
//!
//! 判据映射（锚点原文 → 自检项）：
//! - **O01 架构声明** → `O07-规格-*`（规格表可机检、七类封闭全集、
//!   平行数组对账、声明承载语义、零 panic 面）
//! - **集成边界** → `O07-边界-*`（上游哈希对账双向、下游前向声明、
//!   消费字段面与 F2806 承诺逐位对齐、对账钩子正反向）
//! - **解析子集** → `O07-子集-*`（索引查回、同名源序、墓碑语义、
//!   七类逐一入账、承接展开产物名字与值双层对拍、非承载类钳空）
//! - **降级矩阵** → `O07-降级-*`（非法输入→拒绝三要素 / 边界越界→
//!   钳制+告警 / 账满→拒绝不覆盖 / 异常→立案流转 / 统计守恒）
//! - **判据** → `O07-判据-*`（零 panic 自扫描、独立第二套查找实现对拍、
//!   反向语料可分辨、码表冻结、非递归容量断言）
//!
//! **判据设计纪律**（承 F2806 八条 + 本单新增一条）：
//! 1. 不向被测函数问答案——期望值判据侧独立算出（longhand 24 名是
//!    **独立写死的字面量清单**，不从 `LONGHAND_TABLE` 反推）；
//! 2. 双向验证——每条拒绝判据配对应接受判据；
//! 3. 形状与值两层都断；
//! 4. 专属错误码（断「有东西坏了」等于没断）；
//! 5. 弱门禁须指认它能抓的变异；
//! 6. 判据区零 panic 面；
//! 9. **判据不得自调全域入口**（F2806 判据-06 教训：B 族→全域→B 族
//!    无限递归栈溢出）——本单容量断言走「独立重建 A 族 + 本集 tally」。

use crate::checks::{CheckSet, MAX_CHECKS};

use super::veo05_props::{Declaration, ParsedValue, PropertyId};
use super::veo06_shorthand::{self, Edge, Expansion, LonghandDecl, ShorthandExpander};
use super::veo07_rules::*;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧常量与语料（不从被测常量推导）
// ---------------------------------------------------------------------------

const EXPECT_CODES: usize = 7;

/// 判据侧**独立写死**的 F2806 longhand 24 名全集（字面量，不反推）。
const KNOWN_LONGHANDS: [&str; 24] = [
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "border-top-width",
    "border-right-width",
    "border-bottom-width",
    "border-left-width",
    "border-top-style",
    "border-right-style",
    "border-bottom-style",
    "border-left-style",
    "border-top-color",
    "border-right-color",
    "border-bottom-color",
    "border-left-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
];

/// 判据侧独立写死的码表（0x2Axx 段冻结）。
const EXPECT_WIRE: [&str; EXPECT_CODES] = [
    "E_SHEET_NAME_EMPTY",
    "E_SHEET_KIND_INVALID",
    "E_SHEET_CAP",
    "E_SHEET_LONGHAND_UNKNOWN",
    "E_SHEET_ID_INVALID",
    "E_SHEET_UPSTREAM_DRIFT",
    "E_SHEET_DOWNSTREAM_DRIFT",
];

fn kw(s: &str) -> ParsedValue {
    ParsedValue::Keyword(String::from(s))
}

/// 判据侧取一个合法 PropertyId（零 panic：循环秩位至命中；
/// None 分支被 F2805 四族非空编译期闸排除，仍显式循环闭合）。
fn any_free_id() -> PropertyId {
    let mut r: u16 = 0;
    loop {
        if let Some(id) = PropertyId::of_rank(r) {
            return id;
        }
        r = r.saturating_add(1);
    }
}

/// 造一条 F2805 声明（不走解析器——本单判据只关心存储与承接）。
fn make_decl(name: &'static str, items: &[&str], important: bool, offset: u32) -> Declaration {
    let mut v: Vec<ParsedValue> = Vec::new();
    for it in items.iter() {
        v.push(kw(it));
    }
    Declaration {
        id: any_free_id(),
        name,
        value: ParsedValue::List(v),
        important,
        offset,
    }
}

/// 借**真 F2806 展开器**产一份 margin 展开产物（集成真话）。
fn expand_margin(vals: &[&str], important: bool, offset: u32) -> Expansion {
    let mut expander = ShorthandExpander::new();
    let d = make_decl("margin", vals, important, offset);
    match expander.expand(&d, &list_items(vals), &[]) {
        Ok(e) => e,
        Err(_) => Expansion {
            shorthand: String::new(),
            longhands: Vec::new(),
            dual_axis: false,
            offset: 0,
        },
    }
}

fn list_items(items: &[&str]) -> Vec<ParsedValue> {
    let mut v = Vec::new();
    for it in items.iter() {
        v.push(kw(it));
    }
    v
}

/// 手工伪造一条含未知 longhand 的展开产物（降级语料）。
fn forged_expansion() -> Expansion {
    let mut longhands: Vec<LonghandDecl> = Vec::new();
    longhands.push(LonghandDecl {
        name: String::from("margin-top"),
        value: kw("1px"),
        edge: Edge::Top,
        important: false,
        offset: 0,
        axis: None,
    });
    longhands.push(LonghandDecl {
        name: String::from("not-a-longhand"),
        value: kw("2px"),
        edge: Edge::Right,
        important: false,
        offset: 0,
        axis: None,
    });
    Expansion {
        shorthand: String::from("forged"),
        longhands,
        dual_axis: false,
        offset: 0,
    }
}

// ---------------------------------------------------------------------------
// 一、规格（架构声明）
// ---------------------------------------------------------------------------

fn chk_spec_audit(set: &mut CheckSet) {
    // 规格-01：规格表审计全过。
    match audit_spec_table() {
        Ok(_) => set.ok("O07-规格-01-规格表审计全过"),
        Err(e) => set.fail("O07-规格-01-规格表审计全过", e.code),
    }
    // 规格-02：摘要自报版本/上限/类名（可机检落点非空串）。
    let msg = spec_summary();
    if msg.contains(SHEET_VERSION) && msg.contains("256") && msg.contains("font-face") {
        set.ok("O07-规格-02-摘要含版本与上限");
    } else {
        set.fail("O07-规格-02-摘要含版本与上限", "摘要缺关键段");
    }
}

fn chk_spec_closed(set: &mut CheckSet) {
    // 规格-03：七类封闭——0..=6 全 Some、7..=10 全 None。
    let mut closed = true;
    let mut i = 0usize;
    while i < 7 {
        if RuleKind::of_rank(i).is_none() {
            closed = false;
        }
        i += 1;
    }
    let mut j = 7usize;
    while j < 11 {
        if RuleKind::of_rank(j).is_some() {
            closed = false;
        }
        j += 1;
    }
    if closed && RULE_KIND_COUNT == 7 {
        set.ok("O07-规格-03-七类封闭全集");
    } else {
        set.fail("O07-规格-03-七类封闭全集", "越界秩可构造或全集数不符");
    }
    // 规格-04：rank 往返 + zh 互异。
    let mut uniq: Vec<&str> = Vec::new();
    let mut ok_rt = true;
    for k in RuleKind::ALL.iter() {
        if RuleKind::of_rank(k.rank()) != Some(*k) {
            ok_rt = false;
        }
        uniq.push(k.zh());
    }
    uniq.sort_unstable();
    uniq.dedup();
    if ok_rt && uniq.len() == RULE_KIND_COUNT {
        set.ok("O07-规格-04-秩往返且中文名互异");
    } else {
        set.fail("O07-规格-04-秩往返且中文名互异", "往返失败或重名");
    }
}

fn chk_spec_gates(set: &mut CheckSet) {
    // 规格-05：全域审计三段齐。
    match audit_all() {
        Ok(m) => {
            if m.contains("上游") && m.contains("下游") {
                set.ok("O07-规格-05-全域审计三段齐");
            } else {
                set.fail("O07-规格-05-全域审计三段齐", "缺段");
            }
        }
        Err(e) => set.fail("O07-规格-05-全域审计三段齐", e.code),
    }
    // 规格-06：平行数组与规格表逐位一致（双向对账）。
    let mut bad = 0usize;
    for i in 0..RULE_KIND_COUNT {
        let pm = PARAM_MAX_OF.get(i).copied().unwrap_or(0);
        let dc = DECLS_CAP_OF.get(i).copied().unwrap_or(0);
        let at = AT_RULE_OF.get(i).copied().unwrap_or(false);
        match RULE_SPECS.get(i) {
            Some(sp) if sp.param_max == pm && sp.decls_cap == dc && sp.at_rule == at => {}
            _ => bad += 1,
        }
    }
    if bad == 0 {
        set.ok("O07-规格-06-平行数组与规格表逐位一致");
    } else {
        set.fail("O07-规格-06-平行数组与规格表逐位一致", "有漂移项");
    }
    // 规格-07：声明承载语义（判据侧写死：样式 64、字体面 32、其余 0）。
    let load_ok = DECLS_CAP_OF[0] == 64
        && DECLS_CAP_OF[3] == 32
        && DECLS_CAP_OF[1] == 0
        && DECLS_CAP_OF[2] == 0
        && DECLS_CAP_OF[4] == 0
        && DECLS_CAP_OF[5] == 0
        && DECLS_CAP_OF[6] == 0;
    if load_ok {
        set.ok("O07-规格-07-声明承载恰两类");
    } else {
        set.fail("O07-规格-07-声明承载恰两类", "承载语义漂移");
    }
}

fn chk_spec_zero_panic(set: &mut CheckSet) {
    // 规格-08：生产代码零 panic 面（单遍词法剥除后扫）。
    let src = include_str!("veo07_rules.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("O07-规格-08-零 panic 面");
    } else {
        set.fail("O07-规格-08-零 panic 面", "生产代码含 panic 面");
    }
}

// ---------------------------------------------------------------------------
// 二、边界（跨批对接）
// ---------------------------------------------------------------------------

fn chk_bound_upstream(set: &mut CheckSet) {
    // 边界-01：上游对账通过且对端是 F2806。
    let c = audit_upstream();
    if c.matched && c.peer == "VE-F2806" {
        set.ok("O07-边界-01-上游对账通过");
    } else {
        set.fail("O07-边界-01-上游对账通过", "对账失败或对端错");
    }
    // 边界-02：缺失计数双向可分辨（真摘要 0 缺、劣化摘要必缺）。
    let real = veo06_shorthand::spec_summary();
    let m0 = upstream_missing_for(&real);
    let degraded = real.replace("border-radius", "border-xadius");
    let m1 = upstream_missing_for(&degraded);
    if m0 == 0 && m1 >= 1 {
        set.ok("O07-边界-02-缺失计数双向可分辨");
    } else {
        set.fail("O07-边界-02-缺失计数双向可分辨", "对账函数不可分辨");
    }
    // 边界-03：契约读屏随 matched 翻面（通过/拒绝各出现）。
    let pass_line = c.screen_line();
    let mut fail_contract = c.clone();
    fail_contract.missing = 1;
    fail_contract.matched = false;
    let fail_line = fail_contract.screen_line();
    if pass_line.contains("通过") && fail_line.contains("拒绝") {
        set.ok("O07-边界-03-契约读屏随对账结果翻面");
    } else {
        set.fail("O07-边界-03-契约读屏随对账结果翻面", "读屏不随结果变");
    }
}

fn chk_bound_downstream(set: &mut CheckSet) {
    // 边界-04：下游对端与消费面。
    match audit_downstream() {
        Ok(msg) => {
            if msg.contains("F2808") && msg.contains("rules_in_source_order") {
                set.ok("O07-边界-04-下游前向声明完整");
            } else {
                set.fail("O07-边界-04-下游前向声明完整", "声明缺关键面");
            }
        }
        Err(e) => set.fail("O07-边界-04-下游前向声明完整", e.code),
    }
    // 边界-05：本单正是 F2806 前向声明的消费方（跨模块对齐）。
    if veo06_shorthand::DOWNSTREAM_DECL.peer == "VE-F2807" {
        set.ok("O07-边界-05-F2806前向声明点名本单");
    } else {
        set.fail("O07-边界-05-F2806前向声明点名本单", "上游声明的消费方不是本单");
    }
    // 边界-06：F2806 承诺的消费字段面（判据侧写死）逐位对齐。
    const PROMISED: [&str; 4] = ["longhand_name", "value", "important", "offset"];
    let promised = veo06_shorthand::DOWNSTREAM_DECL.fields;
    let mut align = promised.len() == PROMISED.len();
    if align {
        let mut i = 0usize;
        while i < PROMISED.len() {
            if promised.get(i) != Some(&PROMISED[i]) {
                align = false;
            }
            i += 1;
        }
    }
    if align {
        set.ok("O07-边界-06-消费字段面与上游声明逐位对齐");
    } else {
        set.fail("O07-边界-06-消费字段面与上游声明逐位对齐", "字段面漂移");
    }
}

fn chk_bound_reconcile(set: &mut CheckSet) {
    // 正向：插入一条样式规则后，点名清单对账命中。
    let mut sheet = Stylesheet::new();
    let exp = expand_margin(&["a", "b"], false, 7);
    let id = match sheet.insert_style("div.a", &exp, 7) {
        Ok(id) => id,
        Err(e) => {
            set.fail("O07-边界-07-对账钩子正向", e.code);
            return;
        }
    };
    match reconcile_stylesheet(&sheet, &["div.a"]) {
        Ok(total) => {
            if total == 1 && sheet.rule_of(id).is_some() {
                set.ok("O07-边界-07-对账钩子正向");
            } else {
                set.fail("O07-边界-07-对账钩子正向", "命中数不符");
            }
        }
        Err(e) => set.fail("O07-边界-07-对账钩子正向", e.code),
    }
    // 反向三支：空清单 / 含空名 / 未知名。
    let r0 = reconcile_stylesheet(&sheet, &[]);
    let r1 = reconcile_stylesheet(&sheet, &["div.a", ""]);
    let r2 = reconcile_stylesheet(&sheet, &["div.ghost"]);
    if r0.is_err() && r1.is_err() && r2.is_err() {
        set.ok("O07-边界-08-对账钩子反向三支全拒");
    } else {
        set.fail("O07-边界-08-对账钩子反向三支全拒", "有空转分支放行");
    }
}

// ---------------------------------------------------------------------------
// 三、子集（存储与承接）
// ---------------------------------------------------------------------------

fn chk_subset_index(set: &mut CheckSet) {
    // 子集-01：插入 + 查回（索引 O(1) 语义：按名命中恰插入那条）。
    let mut sheet = Stylesheet::new();
    let exp = expand_margin(&["a"], false, 0);
    let id = match sheet.insert_style("div.a", &exp, 0) {
        Ok(id) => id,
        Err(e) => {
            set.fail("O07-子集-01-插入查回", e.code);
            return;
        }
    };
    let ids = sheet.lookup_ids("div.a");
    if ids.len() == 1 && ids.first().copied() == Some(id) {
        set.ok("O07-子集-01-插入查回");
    } else {
        set.fail("O07-子集-01-插入查回", "索引命中不符");
    }
    // 子集-02：同名规则共容且按源序（CSS 同选择器多条合法）。
    let id2 = match sheet.insert_style("div.a", &exp, 1) {
        Ok(id) => id,
        Err(e) => {
            set.fail("O07-子集-02-同名源序", e.code);
            return;
        }
    };
    let ids2 = sheet.lookup_ids("div.a");
    if ids2.len() == 2 && ids2.first().copied() == Some(id) && ids2.get(1).copied() == Some(id2) {
        set.ok("O07-子集-02-同名共容且源序");
    } else {
        set.fail("O07-子集-02-同名源序", "顺序或数量不符");
    }
}

fn chk_subset_tombstone(set: &mut CheckSet) {
    // 子集-03：墓碑语义——移除后按名不命中、rule_of 不返回、live_count 减。
    let mut sheet = Stylesheet::new();
    let exp = expand_margin(&["a"], false, 0);
    let id = match sheet.insert_style("p.x", &exp, 0) {
        Ok(id) => id,
        Err(e) => {
            set.fail("O07-子集-03-墓碑语义", e.code);
            return;
        }
    };
    let before = sheet.live_count();
    if sheet.remove_rule(id).is_err() {
        set.fail("O07-子集-03-墓碑语义", "合法移除被拒");
        return;
    }
    let gone = sheet.lookup_ids("p.x").is_empty()
        && sheet.rule_of(id).is_none()
        && sheet.live_count() + 1 == before;
    if gone {
        set.ok("O07-子集-03-墓碑语义");
    } else {
        set.fail("O07-子集-03-墓碑语义", "墓碑泄漏或计数漂移");
    }
}

fn chk_subset_all_kinds(set: &mut CheckSet) {
    // 子集-04：七类逐一入账成功（各名各参，封闭全集全可达）。
    let mut sheet = Stylesheet::new();
    let names: [&str; 7] = [
        "div.k",
        "(max-width:1px)",
        "a.css",
        "Serif",
        "spin",
        "http://n",
        "note",
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < RULE_KIND_COUNT {
        let kind = match RuleKind::of_rank(i) {
            Some(k) => k,
            None => {
                ok = false;
                break;
            }
        };
        let decls: Vec<StoredDecl> = Vec::new();
        if sheet.insert_rule(kind, names[i], decls, 0).is_err() {
            ok = false;
        }
        i += 1;
    }
    if ok && sheet.live_count() == RULE_KIND_COUNT {
        set.ok("O07-子集-04-七类逐一入账");
    } else {
        set.fail("O07-子集-04-七类逐一入账", "有类不可达或计数不符");
    }
}

fn chk_subset_carry(set: &mut CheckSet) {
    // 子集-05：承接展开产物——名字逐条（判据侧写死）+ important/offset 承袭。
    let mut sheet = Stylesheet::new();
    let exp = expand_margin(&["a", "b"], true, 42);
    let id = match sheet.insert_style("div.c", &exp, 42) {
        Ok(id) => id,
        Err(e) => {
            set.fail("O07-子集-05-承接名字与承袭", e.code);
            return;
        }
    };
    let want: [&str; 4] = [
        "margin-top",
        "margin-right",
        "margin-bottom",
        "margin-left",
    ];
    let rule = match sheet.rule_of(id) {
        Some(r) => r,
        None => {
            set.fail("O07-子集-05-承接名字与承袭", "规则查无");
            return;
        }
    };
    let mut ok = rule.decls.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        match rule.decls.get(i) {
            Some(dl) => {
                if dl.name != want[i] || !dl.important || dl.offset != 42 {
                    ok = false;
                }
            }
            None => ok = false,
        }
        i += 1;
    }
    if ok {
        set.ok("O07-子集-05-承接名字与承袭");
    } else {
        set.fail("O07-子集-05-承接名字与承袭", "名字或承袭字段不符");
    }
    // 子集-06：2 分量分配值双层对拍（上/下 = a，右/左 = b）。
    let want_vals: [&str; 4] = ["a", "b", "a", "b"];
    let mut val_ok = true;
    let mut i = 0usize;
    while i < 4 {
        match rule.decls.get(i) {
            Some(dl) => match &dl.value {
                ParsedValue::Keyword(s) => {
                    if s.as_str() != want_vals[i] {
                        val_ok = false;
                    }
                }
                _ => val_ok = false,
            },
            None => val_ok = false,
        }
        i += 1;
    }
    if val_ok {
        set.ok("O07-子集-06-承接值逐位对拍");
    } else {
        set.fail("O07-子集-06-承接值逐位对拍", "分配值不符");
    }
}

fn chk_subset_known_longhand(set: &mut CheckSet) {
    // 子集-07：承接校验真值表——判据侧独立写死的 24 名全过、外来名全拒。
    let mut all_known = true;
    for n in KNOWN_LONGHANDS.iter() {
        if !is_known_longhand(n) {
            all_known = false;
        }
    }
    let outsiders: [&str; 5] = ["", "margin-top2", "color", "MARGIN-TOP", "margin"];
    let mut all_unknown = true;
    for n in outsiders.iter() {
        if is_known_longhand(n) {
            all_unknown = false;
        }
    }
    if all_known && all_unknown {
        set.ok("O07-子集-07-longhand全集真值表");
    } else {
        set.fail("O07-子集-07-longhand全集真值表", "24 名清单或外来名判定漂移");
    }
    // 子集-08：伪造展开产物承接被拒且立案（E_SHEET_LONGHAND_UNKNOWN）。
    let mut sheet = Stylesheet::new();
    let bad = forged_expansion();
    let before_cases = sheet.cases.len();
    match sheet.insert_style("div.f", &bad, 0) {
        Ok(_) => set.fail("O07-子集-08-未知longhand拒收立案", "伪造产物竟然入账"),
        Err(e) => {
            let filed = sheet.cases.len() == before_cases + 1;
            if e.code == E_SHEET_LONGHAND_UNKNOWN && filed {
                set.ok("O07-子集-08-未知longhand拒收立案");
            } else {
                set.fail("O07-子集-08-未知longhand拒收立案", e.code);
            }
        }
    }
}

fn chk_subset_nonstyle_clamp(set: &mut CheckSet) {
    // 子集-09：非承载类塞声明被钳空（DECLS_CAP_OF==0 的类声明恒不落账）。
    let mut sheet = Stylesheet::new();
    let mut decls: Vec<StoredDecl> = Vec::new();
    decls.push(StoredDecl {
        name: String::from("margin-top"),
        value: kw("1px"),
        important: false,
        offset: 0,
    });
    match sheet.insert_rule(RuleKind::Media, "(max-width:1px)", decls, 0) {
        Ok(_) => {
            let clamped = sheet.stats.clamped >= 1;
            let empty_decls = sheet
                .rules
                .last()
                .map(|r| r.decls.is_empty())
                .unwrap_or(false);
            if clamped && empty_decls {
                set.ok("O07-子集-09-非承载类声明钳空");
            } else {
                set.fail("O07-子集-09-非承载类声明钳空", "钳制未发生或未记账");
            }
        }
        Err(e) => set.fail("O07-子集-09-非承载类声明钳空", e.code),
    }
}

// ---------------------------------------------------------------------------
// 四、降级矩阵
// ---------------------------------------------------------------------------

fn chk_degrade_rejects(set: &mut CheckSet) {
    // 降级-01：空名拒专属码 + 三要素齐。
    let mut sheet = Stylesheet::new();
    match sheet.insert_rule(RuleKind::Style, "", Vec::new(), 0) {
        Ok(_) => set.fail("O07-降级-01-空名拒三要素", "空名竟然入账"),
        Err(e) => {
            let three = !e.what.is_empty() && !e.why.is_empty() && !e.next.is_empty();
            if e.code == E_SHEET_NAME_EMPTY && three {
                set.ok("O07-降级-01-空名拒三要素");
            } else {
                set.fail("O07-降级-01-空名拒三要素", e.code);
            }
        }
    }
    // 降级-02：枚举守卫——越界秩类型面不可构造。
    let guard = RuleKind::of_rank(7).is_none() && RuleKind::of_rank(usize::MAX).is_none();
    if guard {
        set.ok("O07-降级-02-枚举守卫类型面封死");
    } else {
        set.fail("O07-降级-02-枚举守卫类型面封死", "越界秩可构造");
    }
    // 降级-03：移除不存在 id 拒 + 立案。
    let before = sheet.cases.len();
    match sheet.remove_rule(999) {
        Ok(_) => set.fail("O07-降级-03-移除无效id拒立案", "幽灵 id 竟然成功"),
        Err(e) => {
            let filed = sheet.cases.len() == before + 1;
            if e.code == E_SHEET_ID_INVALID && filed {
                set.ok("O07-降级-03-移除无效id拒立案");
            } else {
                set.fail("O07-降级-03-移除无效id拒立案", e.code);
            }
        }
    }
}

fn chk_degrade_sheet_cap(set: &mut CheckSet) {
    // 降级-04：表满拒且不覆盖旧行（守恒：MAX_RULES 接受 + 1 拒绝）。
    let mut sheet = Stylesheet::new();
    let mut last_err: Option<&'static str> = None;
    let mut i = 0usize;
    while i <= MAX_RULES {
        let name = format!("r{}", i);
        match sheet.insert_rule(RuleKind::Style, &name, Vec::new(), 0) {
            Ok(_) => {}
            Err(e) => {
                last_err = Some(e.code);
                break;
            }
        }
        i += 1;
    }
    let cap_ok = last_err == Some(E_SHEET_CAP)
        && sheet.live_count() == MAX_RULES
        && sheet.stats.accepted == MAX_RULES as u32
        && sheet.stats.rejected == 1
        && sheet.stats.conserves();
    if cap_ok {
        set.ok("O07-降级-04-表满拒不覆盖");
    } else {
        set.fail("O07-降级-04-表满拒不覆盖", "容量语义漂移");
    }
    // 降级-05：首条仍在（不覆盖的实质证据）。
    if sheet.lookup_ids("r0").len() == 1 {
        set.ok("O07-降级-05-满后首条仍在");
    } else {
        set.fail("O07-降级-05-满后首条仍在", "旧行被覆盖");
    }
}

fn chk_degrade_clamps(set: &mut CheckSet) {
    // 降级-06：名字超上界 → 钳制 + 告警 + 钳后名可查回。
    let mut sheet = Stylesheet::new();
    let long = format!("div.{}", "x".repeat(400));
    match sheet.insert_rule(RuleKind::Style, &long, Vec::new(), 0) {
        Ok(id) => {
            let clamped_cap = sheet
                .rule_of(id)
                .map(|r| r.name.len() == 256)
                .unwrap_or(false);
            let warned = sheet.clamps.len() == 1 && sheet.stats.clamped == 1;
            let findable = sheet.lookup_ids(&long[..256]).len() == 1;
            if clamped_cap && warned && findable {
                set.ok("O07-降级-06-名字钳制告警可查回");
            } else {
                set.fail("O07-降级-06-名字钳制告警可查回", "钳制或告警不符");
            }
        }
        Err(e) => set.fail("O07-降级-06-名字钳制告警可查回", e.code),
    }
    // 降级-07：声明数超类上界 → 钳收 + 记账（70 → 64）。
    let mut decls: Vec<StoredDecl> = Vec::new();
    let mut i = 0usize;
    while i < 70 {
        decls.push(StoredDecl {
            name: String::from("margin-top"),
            value: kw("1px"),
            important: false,
            offset: i as u32,
        });
        i += 1;
    }
    match sheet.insert_rule(RuleKind::Style, "div.d", decls, 0) {
        Ok(id) => {
            let kept = sheet
                .rule_of(id)
                .map(|r| r.decls.len() == 64)
                .unwrap_or(false);
            if kept && sheet.stats.clamped >= 2 {
                set.ok("O07-降级-07-声明超界钳收记账");
            } else {
                set.fail("O07-降级-07-声明超界钳收记账", "钳收或记账不符");
            }
        }
        Err(e) => set.fail("O07-降级-07-声明超界钳收记账", e.code),
    }
}

fn chk_degrade_stats(set: &mut CheckSet) {
    // 降级-08：统计守恒（多操作后 accepted+rejected==attempts）。
    let mut sheet = Stylesheet::new();
    let _ = sheet.insert_rule(RuleKind::Style, "div.a", Vec::new(), 0);
    let _ = sheet.insert_rule(RuleKind::Style, "", Vec::new(), 0);
    let _ = sheet.remove_rule(42);
    let _ = sheet.insert_rule(RuleKind::Comment, "note", Vec::new(), 0);
    let cons = sheet.stats.conserves();
    let produced_ok = sheet.stats.produced == 0;
    if cons && produced_ok {
        set.ok("O07-降级-08-统计守恒");
    } else {
        set.fail("O07-降级-08-统计守恒", "守恒或计数漂移");
    }
    // 降级-09：produced == 在账声明实和（独立重扫）。
    let exp = expand_margin(&["a"], false, 0);
    let _ = sheet.insert_style("div.b", &exp, 0);
    let mut sum = 0usize;
    for r in sheet.rules.iter() {
        if !r.removed {
            sum += r.decls.len();
        }
    }
    if sum as u32 == sheet.stats.produced && sheet.stats.produced == 4 {
        set.ok("O07-降级-09-produced独立重扫对账");
    } else {
        set.fail("O07-降级-09-produced独立重扫对账", "声明总账不符");
    }
}

fn chk_degrade_codes(set: &mut CheckSet) {
    // 降级-10：七码互异且非空。
    let codes: [&str; EXPECT_CODES] = [
        E_SHEET_NAME_EMPTY,
        E_SHEET_KIND_INVALID,
        E_SHEET_CAP,
        E_SHEET_LONGHAND_UNKNOWN,
        E_SHEET_ID_INVALID,
        E_SHEET_UPSTREAM_DRIFT,
        E_SHEET_DOWNSTREAM_DRIFT,
    ];
    let mut uniq: Vec<&str> = codes.to_vec();
    uniq.sort_unstable();
    uniq.dedup();
    let mut nonempty = true;
    for c in codes.iter() {
        if c.is_empty() {
            nonempty = false;
        }
    }
    if uniq.len() == EXPECT_CODES && nonempty {
        set.ok("O07-降级-10-七码互异非空");
    } else {
        set.fail("O07-降级-10-七码互异非空", "码重复或为空");
    }
    // 降级-11：码表冻结（判据侧独立字面量表逐位一致）。
    let mut got: Vec<&str> = codes.to_vec();
    got.sort_unstable();
    let mut want: Vec<&str> = EXPECT_WIRE.to_vec();
    want.sort_unstable();
    if got == want {
        set.ok("O07-降级-11-码表冻结一致");
    } else {
        set.fail("O07-降级-11-码表冻结一致", "码表漂移");
    }
    // 降级-12：变体——码表任一位改掉须与实测不同（否则冻结判据恒真）。
    let mutated = {
        let mut w = EXPECT_WIRE.to_vec();
        w[0] = "E_SHEET_MUTATED";
        let mut sw = w.to_vec();
        sw.sort_unstable();
        let mut sa: Vec<&str> = codes.to_vec();
        sa.sort_unstable();
        sa != sw
    };
    if mutated {
        set.ok("O07-降级-12-变体码表漂移必被抓");
    } else {
        set.fail("O07-降级-12-变体码表漂移必被抓", "冻结判据失去分辨力");
    }
}

fn chk_degrade_screen(set: &mut CheckSet) {
    // 降级-13：读屏可达——规则行与表摘要含关键事实。
    let mut sheet = Stylesheet::new();
    let exp = expand_margin(&["a", "b"], false, 3);
    let _ = sheet.insert_style("div.s", &exp, 3);
    let text = sheet.screen_text();
    let ok = text.contains("样式表")
        && text.contains("div.s")
        && text.contains("margin-top")
        && text.contains("在账 1 条");
    if ok {
        set.ok("O07-降级-13-读屏含关键事实");
    } else {
        set.fail("O07-降级-13-读屏含关键事实", "读屏缺关键信息");
    }
    // 降级-14：每次拒绝都立案且不多立案（3 连拒 3 案）。
    let mut sheet2 = Stylesheet::new();
    let _ = sheet2.insert_rule(RuleKind::Style, "", Vec::new(), 0);
    let _ = sheet2.remove_rule(77);
    let bad = forged_expansion();
    let _ = sheet2.insert_style("div.f", &bad, 0);
    let filed = sheet2.cases.len() == 3 && sheet2.stats.rejected == 3;
    if filed {
        set.ok("O07-降级-14-拒绝与立案一比一");
    } else {
        set.fail("O07-降级-14-拒绝与立案一比一", "立案缺失或虚增");
    }
}

// ---------------------------------------------------------------------------
// 五、判据承载力
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据层零 panic 面（单遍词法剥除后扫本文件）。
    let src = include_str!("veo07_rules_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("O07-判据-01-判据层零 panic 面");
    } else {
        set.fail("O07-判据-01-判据层零 panic 面", "判据层含 panic 面");
    }
}

fn chk_criterion_no_empty(set: &mut CheckSet) {
    // 判据-02：期望名单不含空串（24 名 + 规格名 + 消费面）。
    let mut empty = 0usize;
    for n in KNOWN_LONGHANDS.iter() {
        if n.is_empty() {
            empty += 1;
        }
    }
    for sp in RULE_SPECS.iter() {
        if sp.name.is_empty() {
            empty += 1;
        }
    }
    for f in DOWNSTREAM_DECL.faces.iter() {
        if f.is_empty() {
            empty += 1;
        }
    }
    if empty == 0 {
        set.ok("O07-判据-02-期望名单无空串");
    } else {
        set.fail("O07-判据-02-期望名单无空串", "名单含空串（命中恒真）");
    }
}

fn chk_criterion_independent_index(set: &mut CheckSet) {
    // 判据-03：独立第二套查找实现与被测索引逐位一致。
    // 判据侧线性扫（不调 NameIndex），在 6 名语料上按名对拍。
    let mut sheet = Stylesheet::new();
    let corpus: [&str; 6] = ["a", "b", "a", "c", "b", "a"];
    let mut expect: Vec<(&str, u64)> = Vec::new();
    let mut i = 0usize;
    for n in corpus.iter() {
        if sheet.insert_rule(RuleKind::Style, n, Vec::new(), i as u32).is_ok() {
            expect.push((n, (i + 1) as u64));
        }
        i += 1;
    }
    // 判据侧独立线性映射：名字 → id 序列（按插入序）。
    let mut seen: Vec<&str> = Vec::new();
    for (n, _) in expect.iter() {
        if !seen.iter().any(|s| s == n) {
            seen.push(n);
        }
    }
    let mut mismatch = 0usize;
    for n in seen.iter() {
        let want: Vec<u64> = expect
            .iter()
            .filter(|(name, _)| name == n)
            .map(|(_, id)| *id)
            .collect();
        let got = sheet.lookup_ids(n);
        if got != want {
            mismatch += 1;
        }
    }
    if mismatch == 0 {
        set.ok("O07-判据-03-独立查找实现对拍一致");
    } else {
        set.fail("O07-判据-03-独立查找实现对拍一致", "两套实现不一致");
    }
}

fn chk_criterion_reverse(set: &mut CheckSet) {
    // 判据-04：反向语料可分辨——劣化名字必须被承接校验拒（双向）。
    let real = KNOWN_LONGHANDS;
    let mut degraded: [&str; 24] = real;
    degraded[0] = "margin-top-x";
    let differs = degraded[0] != real[0]
        && is_known_longhand(real[0])
        && !is_known_longhand(degraded[0]);
    if differs {
        set.ok("O07-判据-04-反向语料可分辨");
    } else {
        set.fail("O07-判据-04-反向语料可分辨", "对拍不可分辨");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-05：容量承载——**不得自调全域入口**（F2806 判据-06 教训：
    // B 族自调全域构成无限递归）。独立重建 A 族，连同本集台账合计断。
    let a = run_veo07_checks_a_standalone();
    let (ap, af) = a.tally();
    let (bp, bf) = set.tally();
    let total = ap + af + bp + bf + 1;
    if !a.truncated()
        && a.dropped() == 0
        && !set.truncated()
        && set.dropped() == 0
        && total <= MAX_CHECKS
    {
        set.ok("O07-判据-05-判据集未截断未丢条");
    } else {
        set.fail("O07-判据-05-判据集未截断未丢条", "判据集被截断或丢条");
    }
}

// ---------------------------------------------------------------------------
// 剥注释与字符串（手写状态机；判据自己写的串不误伤）
// ---------------------------------------------------------------------------

/// 单遍词法剥除：按真实词法序同时处理行注释、块注释（Rust 可嵌套）、
/// 字符串与字符字面量，仅保留 Normal 态字符。
/// 取代旧的三段式（剥块注释→剥行注释→剥字符串）：旧序会把字符串内的
/// `//`、`/*` 误当注释起点，闭合引号被连删后引号错配，整段字符串内容
/// 泄漏成"伪代码"（F2807 实测：语料 "http://n" 触发自伤假红）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        // 行注释：吞到行尾（换行保留，出错行号可追）。
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        // 块注释：按 Rust 语义可嵌套，按深度吞。
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
        // 字符串字面量：整体吞（含转义；字节扫描对 UTF-8 续字节安全，
        // 续字节不会是 ASCII 引号或反斜杠）。
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
        // 字符字面量：整体吞（避免 '\"' 一类扰乱后续状态）。
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
// 入口（a=规格+边界+子集 / b=降级+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 子集。
pub fn run_veo07_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veo07/a");
    chk_spec_audit(&mut s);
    chk_spec_closed(&mut s);
    chk_spec_gates(&mut s);
    chk_spec_zero_panic(&mut s);
    chk_bound_upstream(&mut s);
    chk_bound_downstream(&mut s);
    chk_bound_reconcile(&mut s);
    chk_subset_index(&mut s);
    chk_subset_tombstone(&mut s);
    chk_subset_all_kinds(&mut s);
    chk_subset_carry(&mut s);
    chk_subset_known_longhand(&mut s);
    chk_subset_nonstyle_clamp(&mut s);
    s
}

/// 判据族 b：降级矩阵 + 判据承载力。
pub fn run_veo07_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/veo07/b");
    chk_degrade_rejects(&mut s);
    chk_degrade_sheet_cap(&mut s);
    chk_degrade_clamps(&mut s);
    chk_degrade_stats(&mut s);
    chk_degrade_codes(&mut s);
    chk_degrade_screen(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_no_empty(&mut s);
    chk_criterion_independent_index(&mut s);
    chk_criterion_reverse(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_veo07_checks() -> CheckSet {
    CheckSet::merge(
        run_veo07_checks_a_standalone(),
        run_veo07_checks_b_standalone(),
    )
}
