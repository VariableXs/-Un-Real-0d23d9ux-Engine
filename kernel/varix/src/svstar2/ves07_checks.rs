//! VE-F3607 · 域自检（判据逐条对应，见 `ves07_validator.rs` 头注）
//!
//! 判据映射（锚点原文六条 → 自检族）：
//! - 五段验证（schema 复用/闭合/许可/分级/扫描）→ `P01`~`P05` + `P06` 流水
//! - 引用闭合（O(引用)）→ `P02`
//! - 许可复用（F2919 复述）→ `P03`
//! - 自检分级（降级显性）→ `P04`
//! - 单源复述（F2919/F3303 复述件）→ `P03`/`P05`
//! - 判据承载与家族自洽 → `P06`
//!
//! **判据设计自律**（承 vel09/F2208 教训）：
//! ① 闭合判据配**线性扫描独立对拍**（不调被测 `contains`——二分写错时
//!    判据侧线性扫描仍给出独立期望）；
//! ② 分级期望由判据侧**独立按布尔对数重算**（不调被测 `a11y_grade`）；
//! ③ 立案红线（降级未显性/阻断无理由）配**手工重组的反向语料**——
//!    被测构造不可达的态必须由判据侧手工拼出来证明红线真的在承重；
//! ④ 负向断言配正向计数（缺引用除了断码还要断 missing_refs 恰等于差集）；
//! ⑤ 判据区零 panic 面：取值一律 `.get()`/`match` 记红。

use alloc::vec;
use alloc::vec::Vec;

use super::veq04_type::{ALL_TYPES, ResourceType};
use super::ves07_validator::*;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 语料构造（判据侧自持）
// ---------------------------------------------------------------------------

/// 全绿资产（其余字段按需覆盖）。
fn good_asset(id: u32) -> AssetCandidate {
    AssetCandidate {
        asset_id: id,
        declared_type_en: Some(alloc::string::String::from("texture")),
        declared_type_wire: None,
        refs: Vec::new(),
        license: LicenseRecord { present: true, covers_distribution: true, valid: true },
        a11y: A11ySelfCheck { contrast_ok: true, semantic_labeled: true },
        scan_findings: 0,
    }
}

/// 发布集合 {1,2,3}。
fn set123() -> PublishSet {
    PublishSet::new(vec![1, 2, 3])
}

/// 判据侧独立重算：无障碍评级（按布尔对数，不调被测）。
fn expect_grade(contrast_ok: bool, semantic_labeled: bool) -> (A11yGrade, usize) {
    let n = (if contrast_ok { 0 } else { 1 }) + (if semantic_labeled { 0 } else { 1 });
    let g = match n {
        0 => A11yGrade::A,
        1 => A11yGrade::B,
        _ => A11yGrade::C,
    };
    (g, n)
}

/// 判据侧独立线性包含（对拍用，不调被测 contains）。
fn linear_contains(ids: &PublishSet, id: u32) -> bool {
    // PublishSet 无枚举口，判据侧自存一份语料重扫。
    let _ = ids;
    let corpus: [u32; 3] = [1, 2, 3];
    let mut found = false;
    let mut i = 0usize;
    while i < corpus.len() {
        if let Some(v) = corpus.get(i) {
            if *v == id {
                found = true;
            }
        }
        i += 1;
    }
    found
}

// ---------------------------------------------------------------------------
// P01 · schema 校验（真复用 F3204 单源）
// ---------------------------------------------------------------------------

fn p01_schema() -> CheckSet {
    let mut s = CheckSet::new("VE-F3607-p01");
    let mut bag = DiagBag::new();
    let reg = f3204_registry();

    // 真复用对拍一：十类 en 名全部可解析且 wire 往返一致（用被测单源
    // 的枚举本身做语料——en/wire 映射互恰是契约本体）。
    let mut all_resolve = true;
    let mut i = 0usize;
    while i < ALL_TYPES.len() {
        if let Some(t) = ALL_TYPES.get(i) {
            let by_en = ResourceType::from_en(t.en());
            let by_wire = ResourceType::from_wire(t.wire());
            let en_matches = match by_en {
                Some(x) => x.wire() == t.wire(),
                None => false,
            };
            if !(en_matches && by_wire == Some(*t) && reg.is_registered(*t)) {
                all_resolve = false;
            }
        }
        i += 1;
    }
    s.add("P01-schema-十类en解析且登记", all_resolve, "from_en/from_wire/注册表三口径互恰");
    s.add("P01-schema-十类闭集", ALL_TYPES.len() == 10, "F3204 十类闭集");
    s.add("P01-schema-标准注册表满", reg.registered == 10, "std_registry 全登记");

    // 未知类型名拒 + 记码。
    let mut a = good_asset(1);
    a.declared_type_en = Some(alloc::string::String::from("no_such_type"));
    let set = set123();
    let r = validate(&a, &set, &reg, &mut bag);
    s.add("P01-schema-未知类型拒", !r.schema_ok && r.verdict == Verdict::Blocked, "解析不出即拒");
    s.add("P01-schema-未知类型记码", bag.has(V07Code::SCHEMA_TYPE_UNKNOWN), "拒绝零静默");
    s.add("P01-schema-未知类型阻断理由", r.blocked_reasons.len() == 1, "恰 schema 一条理由");

    // 双口径一致过。
    let mut b = good_asset(1);
    b.declared_type_wire = Some(1); // texture 的 wire（判据侧用被测单源核对下方）
    let mut bag_b = DiagBag::new();
    let rb = validate(&b, &set123(), &reg, &mut bag_b);
    // wire=1 是否 texture 由判据侧独立解析核对（不假设具体数值）。
    let wire1 = ResourceType::from_wire(1);
    match wire1 {
        Some(t) => {
            s.add("P01-schema-双口径一致随wire值", rb.schema_ok == (t.en() == "texture"),
                "一致口径过、漂移口径拒由语料驱动");
        }
        None => s.add("P01-schema-双口径一致随wire值", !rb.schema_ok, "wire 非法即拒"),
    }

    // 双口径漂移拒：en 合法 + wire 指另一类。
    let mut c = good_asset(1);
    c.declared_type_en = Some(alloc::string::String::from("texture"));
    // 判据侧找一个 en 名 != texture 的类型 wire。
    let mut other_wire: Option<u8> = None;
    let mut k = 0usize;
    while k < ALL_TYPES.len() {
        if let Some(t) = ALL_TYPES.get(k) {
            if t.en() != "texture" {
                other_wire = Some(t.wire());
            }
        }
        k += 1;
    }
    match other_wire {
        Some(w) => {
            c.declared_type_wire = Some(w);
            let mut bag_c = DiagBag::new();
            let rc = validate(&c, &set123(), &reg, &mut bag_c);
            s.add("P01-schema-双口径漂移拒", !rc.schema_ok
                && bag_c.has(V07Code::SCHEMA_DUAL_DRIFT), "en/wire 指不同类必须拒");
        }
        None => s.fail("P01-schema-漂移语料", "十类内找不出异类——语料构造失败"),
    }

    // 双口径全缺拒。
    let mut d = good_asset(1);
    d.declared_type_en = None;
    d.declared_type_wire = None;
    let mut bag_d = DiagBag::new();
    let rd = validate(&d, &set123(), &reg, &mut bag_d);
    s.add("P01-schema-全缺拒", !rd.schema_ok && bag_d.has(V07Code::SCHEMA_TYPE_UNKNOWN),
        "无任何声明即拒");

    // 仅 wire 过（en None）。
    let mut e = good_asset(1);
    e.declared_type_en = None;
    e.declared_type_wire = ResourceType::from_en("texture").map(|t| t.wire());
    let mut bag_e = DiagBag::new();
    let re = validate(&e, &set123(), &reg, &mut bag_e);
    s.add("P01-schema-仅wire过", re.schema_ok, "单口径合法声明可过");

    // SCHEMA 码段钉死。
    s.add("P01-schema-码段", V07Code::SCHEMA_TYPE_UNKNOWN.0 >> 8 == 0x37
        && V07Code::SCHEMA_DUAL_DRIFT.0 >> 8 == 0x37, "两码均在 0x37 段");
    s
}

// ---------------------------------------------------------------------------
// P02 · 引用闭合（O(引用·logN)）
// ---------------------------------------------------------------------------

fn p02_refs() -> CheckSet {
    let mut s = CheckSet::new("VE-F3607-p02");
    let reg = f3204_registry();

    // 闭合过：引用 {2,3} 全在集合。
    let mut a = good_asset(1);
    a.refs = vec![2, 3];
    let mut bag = DiagBag::new();
    let r = validate(&a, &set123(), &reg, &mut bag);
    s.add("P02-闭合-全命中过", r.refs_closed && r.missing_refs.is_empty()
        && r.verdict == Verdict::Publishable, "引用全闭合可发布");
    s.add("P02-闭合-过路无码", !bag.has(V07Code::REF_MISSING), "闭合过不记码");

    // 缺失拒 + 清单逐条。
    let mut b = good_asset(1);
    b.refs = vec![2, 3, 99, 100];
    let mut bag_b = DiagBag::new();
    let rb = validate(&b, &set123(), &reg, &mut bag_b);
    s.add("P02-闭合-缺失拒", !rb.refs_closed && rb.verdict == Verdict::Blocked
        && bag_b.has(V07Code::REF_MISSING), "缺失即阻断红线");
    // 缺失清单判据侧独立算差集：{99,100}。
    s.add("P02-闭合-缺失清单恰差集", rb.missing_refs.len() == 2
        && rb.missing_refs.first() == Some(&99) && rb.missing_refs.get(1) == Some(&100),
        "清单=独立差集且有序");
    s.add("P02-闭合-阻断理由在", rb.blocked_reasons.len() >= 1, "阻断带理由");

    // 重复引用同一缺失只记一次。
    let mut c = good_asset(1);
    c.refs = vec![99, 99, 99];
    let mut bag_c = DiagBag::new();
    let rc = validate(&c, &set123(), &reg, &mut bag_c);
    s.add("P02-闭合-重复缺失去重", rc.missing_refs.len() == 1, "一个洞不是三个洞");

    // 自引用（asset_id 在集合内）合法。
    let mut d = good_asset(1);
    d.refs = vec![1, 2];
    let mut bag_d = DiagBag::new();
    let rd = validate(&d, &set123(), &reg, &mut bag_d);
    s.add("P02-闭合-自引用合法", rd.refs_closed, "引用自己所在集合内的 id");

    // 二分 vs 判据侧线性对拍（含边界点）。
    let set = PublishSet::new(vec![1, 2, 3]);
    let probes: [u32; 6] = [1, 2, 3, 0, 4, u32::MAX];
    let mut agree = true;
    let mut pi = 0usize;
    while pi < probes.len() {
        if let Some(p) = probes.get(pi) {
            if set.contains(*p) != linear_contains(&set, *p) {
                agree = false;
            }
        }
        pi += 1;
    }
    s.add("P02-闭合-二分线性对拍", agree, "六探针含首/尾/越界全一致");
    // 乱序输入建集：规模守恒 + 成员全可查。
    let shuffled = PublishSet::new(vec![30, 10, 20, 5, 99]);
    s.add("P02-闭合-乱序建集守恒", shuffled.len() == 5 && shuffled.contains(5)
        && shuffled.contains(99) && shuffled.contains(10) && !shuffled.contains(7),
        "排序建集不改成员");
    // 空集：任何引用都缺失。
    let empty = PublishSet::new(vec![]);
    s.add("P02-闭合-空集全缺失", empty.is_empty() && !empty.contains(1), "空集含零元素");
    // REF_MISSING 码段。
    s.add("P02-闭合-码段", V07Code::REF_MISSING.0 >> 8 == 0x37, "0x37 段");
    s
}

// ---------------------------------------------------------------------------
// P03 · 许可三查（F2919 复述）
// ---------------------------------------------------------------------------

fn p03_license() -> CheckSet {
    let mut s = CheckSet::new("VE-F3607-p03");
    let reg = f3204_registry();
    let set = set123();

    // 全真 → 过。
    let a = good_asset(1);
    let mut bag = DiagBag::new();
    let r = validate(&a, &set, &reg, &mut bag);
    s.add("P03-许可-三查全真过", r.license_ok && !bag.has(V07Code::LICENSE_MISSING)
        && !bag.has(V07Code::LICENSE_SCOPE) && !bag.has(V07Code::LICENSE_EXPIRED),
        "三谓词全真");

    // 三查逐条独立红。
    let mut m = good_asset(1);
    m.license.present = false;
    let mut bag_m = DiagBag::new();
    let rm = validate(&m, &set, &reg, &mut bag_m);
    s.add("P03-许可-缺记录", !rm.license_ok && rm.verdict == Verdict::Blocked
        && bag_m.has(V07Code::LICENSE_MISSING), "查一：记录缺失");

    let mut sc = good_asset(1);
    sc.license.covers_distribution = false;
    let mut bag_sc = DiagBag::new();
    let rsc = validate(&sc, &set, &reg, &mut bag_sc);
    s.add("P03-许可-不覆盖分发", !rsc.license_ok && rsc.verdict == Verdict::Blocked
        && bag_sc.has(V07Code::LICENSE_SCOPE), "查二：范围不覆盖");

    let mut ex = good_asset(1);
    ex.license.valid = false;
    let mut bag_ex = DiagBag::new();
    let rex = validate(&ex, &set, &reg, &mut bag_ex);
    s.add("P03-许可-失效", !rex.license_ok && rex.verdict == Verdict::Blocked
        && bag_ex.has(V07Code::LICENSE_EXPIRED), "查三：失效/过期");

    // 三项全缺 → 三码齐发（独立谓词不互掩）。
    let mut all = good_asset(1);
    all.license = LicenseRecord::default();
    let mut bag_all = DiagBag::new();
    let _ = validate(&all, &set, &reg, &mut bag_all);
    s.add("P03-许可-三码齐发", bag_all.has(V07Code::LICENSE_MISSING)
        && bag_all.has(V07Code::LICENSE_SCOPE) && bag_all.has(V07Code::LICENSE_EXPIRED),
        "三条独立记账");

    // three_queries 顺序契约。
    let lq = LicenseRecord { present: false, covers_distribution: true, valid: false };
    let q = lq.three_queries();
    s.add("P03-许可-三查顺序", q.first() == Some(&false) && q.get(1) == Some(&true)
        && q.get(2) == Some(&false), "序=[present,covers,valid]");

    // 段间隔离：许可红不产生缺失引用。
    s.add("P03-许可-段间隔离", rm.missing_refs.is_empty(), "许可段不写闭合字段");
    // 复述件。
    s.add("P03-许可-复述件指名", F2919_RESTATED_NOTE.contains("F2919")
        && F2919_RESTATED_NOTE.len() > 20, "复述非空且指名 F2919");
    // 码段。
    s.add("P03-许可-码段", V07Code::LICENSE_MISSING.0 >> 8 == 0x37
        && V07Code::LICENSE_SCOPE.0 >> 8 == 0x37 && V07Code::LICENSE_EXPIRED.0 >> 8 == 0x37,
        "三码均在 0x37 段");
    s
}

// ---------------------------------------------------------------------------
// P04 · 无障碍分级（降级显性，不阻断）
// ---------------------------------------------------------------------------

fn p04_a11y() -> CheckSet {
    let mut s = CheckSet::new("VE-F3607-p04");
    let reg = f3204_registry();
    let set = set123();

    // 四象限：判据侧独立重算 grade 对拍。
    let matrix: [(bool, bool); 4] = [
        (true, true),
        (false, true),
        (true, false),
        (false, false),
    ];
    let mut mi = 0usize;
    while mi < matrix.len() {
        let (c_ok, s_ok) = match matrix.get(mi) {
            Some(v) => *v,
            None => (true, true),
        };
        let mut a = good_asset(1);
        a.a11y = A11ySelfCheck { contrast_ok: c_ok, semantic_labeled: s_ok };
        let mut bag = DiagBag::new();
        let r = validate(&a, &set, &reg, &mut bag);
        let (want_grade, want_n) = expect_grade(c_ok, s_ok);
        let reasons_n = r.a11y.reasons.len();
        let name_ok = match mi {
            0 => "P04-分级-全过A",
            1 => "P04-分级-缺对比度B",
            2 => "P04-分级-缺语义B",
            _ => "P04-分级-全缺C",
        };
        s.add(name_ok, r.a11y.grade == want_grade && reasons_n == want_n,
            "评级与原因数=独立重算");
        // 降级 ⇔ 原因非空（同源正反向一起钉）。
        s.add("P04-分级-降级原因同源", (r.a11y.grade != A11yGrade::A) == (reasons_n > 0),
            "grade<A ⇔ reasons 非空");
        // 原因顺序固定：Contrast 先于 Semantic。
        if reasons_n == 2 {
            s.add("P04-分级-原因固定序", r.a11y.reasons.first() == Some(&DowngradeReason::Contrast)
                && r.a11y.reasons.get(1) == Some(&DowngradeReason::Semantic),
                "Contrast 先于 Semantic");
        }
        mi += 1;
    }

    // 降级记显性码（Minor）。
    let mut dg = good_asset(1);
    dg.a11y.contrast_ok = false;
    let mut bag_dg = DiagBag::new();
    let _ = validate(&dg, &set, &reg, &mut bag_dg);
    s.add("P04-分级-降级记码", bag_dg.has(V07Code::A11Y_DOWNGRADE), "降级显性记账");
    s.add("P04-分级-降级不记P1", bag_dg.count_severity(Severity::P1) == 0
        && bag_dg.count_severity(Severity::P0) == 0, "正常降级不是立案");

    // **降评级不阻断**（锚点红线）：grade C 资产其余全过 → Publishable。
    let mut worst = good_asset(1);
    worst.a11y = A11ySelfCheck { contrast_ok: false, semantic_labeled: false };
    let mut bag_w = DiagBag::new();
    let rw = validate(&worst, &set, &reg, &mut bag_w);
    s.add("P04-分级-降级不阻断", rw.verdict == Verdict::Publishable && rw.a11y.grade == A11yGrade::C,
        "C 级仍可发布（分级处置）");
    s.add("P04-分级-裁决不带理由", rw.blocked_reasons.is_empty(), "可发布即零理由");

    // Ord 语义：A < B < C（评级可比是消费端排序的前提）。
    s.add("P04-分级-评级有序", A11yGrade::A < A11yGrade::B && A11yGrade::B < A11yGrade::C,
        "Ord 派生方向钉死");

    // **反向语料**：手工重组「降级未显性」报告 → 立案 P1。
    let silent = ValidationReport {
        schema_ok: true,
        refs_closed: true,
        missing_refs: Vec::new(),
        license_ok: true,
        a11y: A11yOutcome { grade: A11yGrade::B, reasons: Vec::new() },
        scan_clean: true,
        verdict: Verdict::Publishable,
        blocked_reasons: Vec::new(),
    };
    let mut bag_s = DiagBag::new();
    let cases1 = validate_report_consistency(&silent, &mut bag_s);
    s.add("P04-分级-降级未显性立案", cases1 == 1 && bag_s.has(V07Code::A11Y_DOWNGRADE_SILENT)
        && bag_s.count_severity(Severity::P1) == 1, "评级<B 而原因空必须立案");

    // 正常报告零立案。
    let mut bag_ok = DiagBag::new();
    s.add("P04-分级-正常零立案", validate_report_consistency(&rw, &mut bag_ok) == 0,
        "一致报告不误报");

    // 码段。
    s.add("P04-分级-码段", V07Code::A11Y_DOWNGRADE.0 >> 8 == 0x37
        && V07Code::A11Y_DOWNGRADE_SILENT.0 >> 8 == 0x37, "0x37 段");
    s
}

// ---------------------------------------------------------------------------
// P05 · 安全扫描（F3303 复述，检出即 P0）
// ---------------------------------------------------------------------------

fn p05_scan() -> CheckSet {
    let mut s = CheckSet::new("VE-F3607-p05");
    let reg = f3204_registry();
    let set = set123();

    // 干净过。
    let a = good_asset(1);
    let mut bag = DiagBag::new();
    let r = validate(&a, &set, &reg, &mut bag);
    s.add("P05-扫描-干净过", r.scan_clean && r.verdict == Verdict::Publishable
        && !bag.has(V07Code::SCAN_HIT), "零检出干净");

    // 检出阻断 + P0。
    let mut b = good_asset(1);
    b.scan_findings = 3;
    let mut bag_b = DiagBag::new();
    let rb = validate(&b, &set, &reg, &mut bag_b);
    s.add("P05-扫描-检出阻断", !rb.scan_clean && rb.verdict == Verdict::Blocked
        && bag_b.has(V07Code::SCAN_HIT), "检出即阻断");
    s.add("P05-扫描-P0级", bag_b.count_severity(Severity::P0) == 1, "检出=P0（复述红线）");

    // 检出量无关性：1 与 9999 同样阻断。
    let mut c1 = good_asset(1);
    c1.scan_findings = 1;
    let mut c9999 = good_asset(1);
    c9999.scan_findings = 9999;
    let mut bag1 = DiagBag::new();
    let mut bag9999 = DiagBag::new();
    let r1 = validate(&c1, &set, &reg, &mut bag1);
    let r9999 = validate(&c9999, &set, &reg, &mut bag9999);
    s.add("P05-扫描-量无关", r1.verdict == Verdict::Blocked && r9999.verdict == Verdict::Blocked
        && r1.scan_clean == r9999.scan_clean, "检出计数非零即同态");

    // 扫描红不掩盖无障碍评级（段间隔离）。
    s.add("P05-扫描-段间隔离", rb.a11y.grade == A11yGrade::A, "评级仍如实");

    // 复述件。
    s.add("P05-扫描-复述件指名", F3303_RESTATED_NOTE.contains("F3303")
        && F3303_RESTATED_NOTE.len() > 20, "复述非空且指名 F3303");
    s.add("P05-扫描-复述件互异", F2919_RESTATED_NOTE != F3303_RESTATED_NOTE,
        "两单复述件不得同文");
    s.add("P05-扫描-复述件齐备", restated_notes_present(), "双复述齐备谓词真");
    // 码段。
    s.add("P05-扫描-码段", V07Code::SCAN_HIT.0 >> 8 == 0x37, "0x37 段");
    s
}

// ---------------------------------------------------------------------------
// P06 · 流水裁决与家族自洽
// ---------------------------------------------------------------------------

fn p06_pipeline() -> CheckSet {
    let mut s = CheckSet::new("VE-F3607-p06");
    let reg = f3204_registry();
    let set = set123();

    // 全绿资产：Publishable 且零诊断（A 级不 push，各段不 push）。
    let a = good_asset(1);
    let mut bag = DiagBag::new();
    let r = validate(&a, &set, &reg, &mut bag);
    s.add("P06-流水-全绿零诊断", r.verdict == Verdict::Publishable && bag.is_empty()
        && r.blocked_reasons.is_empty(), "全过零记账");
    // 五段字段正交核对。
    s.add("P06-流水-五段正交", r.schema_ok && r.refs_closed && r.license_ok
        && r.a11y.grade == A11yGrade::A && r.scan_clean, "五段全真");

    // 多段同红：理由条数=红段数（每段恰一条）。
    let mut bad = good_asset(1);
    bad.declared_type_en = Some(alloc::string::String::from("nope"));
    bad.refs = vec![77];
    bad.license.present = false;
    bad.scan_findings = 1;
    let mut bag_bad = DiagBag::new();
    let rbad = validate(&bad, &set, &reg, &mut bag_bad);
    s.add("P06-流水-四段同红理由数", rbad.blocked_reasons.len() == 4
        && rbad.verdict == Verdict::Blocked, "理由条数=阻断段数");
    s.add("P06-流水-四段同红字段", !rbad.schema_ok && !rbad.refs_closed
        && !rbad.license_ok && !rbad.scan_clean, "四段字段全如实");

    // **反向语料**：手工重组「阻断无理由」报告 → 立案。
    let mut no_reason = ValidationReport {
        schema_ok: false,
        refs_closed: true,
        missing_refs: Vec::new(),
        license_ok: true,
        a11y: A11yOutcome { grade: A11yGrade::A, reasons: Vec::new() },
        scan_clean: true,
        verdict: Verdict::Blocked,
        blocked_reasons: Vec::new(),
    };
    let _ = &mut no_reason;
    let mut bag_nr = DiagBag::new();
    let cases2 = validate_report_consistency(&no_reason, &mut bag_nr);
    s.add("P06-流水-阻断无理由立案", cases2 == 1 && bag_nr.has(V07Code::BLOCK_NO_REASON),
        "Blocked 空理由必须立案");

    // 家族自洽：10 码、标签互异、码段统一、未登记码兜底。
    s.add("P06-家族-码数", V07Code::ALL.len() == 10, "十码齐");
    s.add("P06-家族-标签互异", labels_unique(), "十码人话互异");
    s.add("P06-家族-码段统一", codes_in_own_segment(), "全部码在 0x37 段");
    s.add("P06-家族-未登记码兜底", V07Code(0x37FF).label() == "未登记诊断码", "未知码不 panic");

    // 诊断袋：计数独立、严重级计数、渲染。
    let mut bag_r = DiagBag::new();
    bag_r.push(V07Code::REF_MISSING, Severity::Major);
    bag_r.push(V07Code::REF_MISSING, Severity::Major);
    bag_r.push(V07Code::SCAN_HIT, Severity::P0);
    s.add("P06-家族-独立计数", bag_r.count(V07Code::REF_MISSING) == 2
        && bag_r.count(V07Code::SCAN_HIT) == 1, "两码各自计数");
    s.add("P06-家族-严重级计数", bag_r.count_severity(Severity::P0) == 1
        && bag_r.count_severity(Severity::Major) == 2, "P0/Major 分列");
    s.add("P06-家族-渲染非空", !bag_r.render().is_empty() && bag_r.render().contains("P0"),
        "render 产出含级别");
    s.add("P06-家族-空袋渲染", DiagBag::new().render().is_empty(), "空袋渲染空串");

    // 冒烟非空。
    let sm = smoke();
    s.add("P06-家族-冒烟", sm.contains("好资产") && sm.contains("坏资产"), "双案例冒烟");
    s
}

// ---------------------------------------------------------------------------
// 聚合
// ---------------------------------------------------------------------------

/// VE-F3607 全量判据（十族合并于 112 上限内）。
pub fn run_ves07_all_checks() -> CheckSet {
    let mut out = CheckSet::new("VE-F3607");
    out = CheckSet::merge(out, p01_schema());
    out = CheckSet::merge(out, p02_refs());
    out = CheckSet::merge(out, p03_license());
    out = CheckSet::merge(out, p04_a11y());
    out = CheckSet::merge(out, p05_scan());
    out = CheckSet::merge(out, p06_pipeline());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_green() {
        let st = run_ves07_all_checks();
        let (items, n) = st.red_items();
        let mut failed = Vec::new();
        for it in items.iter().take(n) {
            if let Some(c) = it {
                if !c.passed {
                    failed.push(c.name);
                }
            }
        }
        assert!(failed.is_empty(), "红项: {:?}", failed);
        assert!(!st.truncated(), "判据数超过 MAX_CHECKS 被截断");
        assert_eq!(st.len(), 71, "判据总数漂移（拆分阈值 112 需重新评估）");
    }
}
