//! CGPU-F2241 域自检（判据逐条映射：优雅哲学/签收/映射/五段/统一编排/不变量/风险）
//!
//! 判据侧独立写死哲学三条款、五主题、十组批次标题、历史五行——同源恒绿的
//! 弱门禁比没有门禁更坏。聚合防自调：A/B 两族 + tally 守恒。

use crate::checks::CheckSet;

use super::vco01_degrade as api;
use super::vco01_degrade::*;

use alloc::string::String;

/// A 族条数（判据侧写死）。
const EXPECT_A_COUNT: usize = 11;
/// B 族防自调 tally 时刻已登记条数（判据侧写死；B 全族 5 条）。
const EXPECT_B_BEFORE: usize = 4;

// ---------------------------------------------------------------------------
// A 族：使命 + 签收 + 映射 + 五段 + 统一编排 + 不变量 + 风险
// ---------------------------------------------------------------------------

fn chk_mission(set: &mut CheckSet) {
    // 规格-01 优雅哲学三条款：字面量独立写死对拍（判据「优雅哲学」）。
    let want = [
        "让性能压力下系统优雅地退而不是难看地崩",
        "每一级降级都是有设计的",
        "可预期、可恢复",
    ];
    let mut i = 0usize;
    let mut ok = PHILOSOPHY_CLAUSES.len() == 3;
    while ok && i < 3 {
        if PHILOSOPHY_CLAUSES[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("E241-规格-01-优雅哲学三条款");
    } else {
        set.fail("E241-规格-01-优雅哲学三条款", "哲学声明漂移");
    }
    // 规格-02 签收：来源含 F2237、七件齐备、signed、指纹现算一致；篡改指纹被拒。
    let receipt_ok = handoff_verify(&N_HANDOFF).is_ok()
        && N_HANDOFF.items.len() == 7
        && N_HANDOFF.signed
        && api::N_HANDOFF_FP != 0;
    let source_ok = N_HANDOFF.source.contains("F2237") && N_HANDOFF.source.contains("N 域");
    let mut tampered = N_HANDOFF;
    tampered.fingerprint = N_HANDOFF_FP ^ 1;
    let tamper_rejected = handoff_verify(&tampered) == Err(C_VCO01_RECEIPT_INVALID);
    if receipt_ok && source_ok && tamper_rejected {
        set.ok("E241-规格-02-移交包签收");
    } else {
        set.fail("E241-规格-02-移交包签收", "签收缺项/指纹漂移或篡改未检出");
    }
}

fn chk_mapping(set: &mut CheckSet) {
    // 规格-03 十组守恒：表内函数 + 判据侧独立重算（区间 2241..2400 每组恰 16）。
    let mut expect_first = 2241u32;
    let mut indep = true;
    let mut i = 0usize;
    while i < GROUP_PLANS.len() {
        let g = &GROUP_PLANS[i];
        if g.first != expect_first || g.last != expect_first + 15 || g.name.is_empty() {
            indep = false;
        }
        expect_first += 16;
        i += 1;
    }
    if indep && groups_conserved() && expect_first == 2401 && O_DOMAIN_TOTAL == 160 {
        set.ok("E241-规格-03-十组守恒");
    } else {
        set.fail("E241-规格-03-十组守恒", "组区间断洞或总数漂移");
    }
    // 规格-04 组名与主题覆盖逐字：判据侧独立写死抽检 + 五主题覆盖双算。
    let name_ok = GROUP_PLANS[0].name == "域开工与降级链总架构组"
        && GROUP_PLANS[1].name == "降级决策引擎组"
        && GROUP_PLANS[2].name == "降级链路全图组"
        && GROUP_PLANS[3].name == "降级可观测与生态组"
        && GROUP_PLANS[9].name == "O 域收口组";
    let want_themes = ["降级模型", "决策引擎", "链路全图", "可观测", "生态"];
    let mut t_ok = OFFICIAL_THEMES.len() == 5;
    for w in want_themes.iter() {
        if !OFFICIAL_THEMES.contains(w) {
            t_ok = false;
        }
    }
    if name_ok && t_ok && themes_covered() {
        set.ok("E241-规格-04-组名主题逐字");
    } else {
        set.fail("E241-规格-04-组名主题逐字", "组名/主题字面量漂移或覆盖缺口");
    }
}

fn chk_stages(set: &mut CheckSet) {
    // 五段-01 单向流水线：逐段前进全通；跳段/回退/原地各显性码拒。
    let mut ok = true;
    let mut s = 0usize;
    while s + 1 < STAGE_NAMES.len() {
        if stage_advance(s, s + 1).is_err() {
            ok = false;
        }
        s += 1;
    }
    ok &= stage_advance(0, 2) == Err(C_VCO01_STAGE_SKIP);
    ok &= stage_advance(1, 3) == Err(C_VCO01_STAGE_SKIP);
    ok &= stage_advance(2, 1) == Err(C_VCO01_STAGE_REWIND);
    ok &= stage_advance(3, 3) == Err(C_VCO01_STAGE_REWIND);
    ok &= stage_advance(4, 5) == Err(C_VCO01_STAGE_SKIP);
    if ok {
        set.ok("E241-五段-01-单向流水线");
    } else {
        set.fail("E241-五段-01-单向流水线", "跳段/回退未被拒绝或正常推进被误拒");
    }
    // 五段-02 枚举映射封闭：五下标互异、段名非空。
    let idx = [
        stage_index(DegradeStage::Trigger),
        stage_index(DegradeStage::Decide),
        stage_index(DegradeStage::Execute),
        stage_index(DegradeStage::Recover),
        stage_index(DegradeStage::Observe),
    ];
    let distinct = idx[0] != idx[1]
        && idx[1] != idx[2]
        && idx[2] != idx[3]
        && idx[3] != idx[4];
    let mut names_ok = true;
    for n in STAGE_NAMES.iter() {
        if n.is_empty() {
            names_ok = false;
        }
    }
    if distinct && names_ok && STAGE_NAMES.len() == 5 {
        set.ok("E241-五段-02-枚举封闭");
    } else {
        set.fail("E241-五段-02-枚举封闭", "枚举映射或段名漂移");
    }
}

fn chk_orchestration(set: &mut CheckSet) {
    // 统一编排-01 历史五行逐行对拍：判据侧独立写死。
    let want = [
        ("D 域", "掉帧链"),
        ("J03", "热阶梯"),
        ("J04", "续航"),
        ("Q02", "流送"),
        ("N01", "效果降级"),
    ];
    let mut ok = LEGACY_DEGRADES.len() == 5;
    let mut i = 0usize;
    while ok && i < 5 {
        if LEGACY_DEGRADES[i].domain != want[i].0 || LEGACY_DEGRADES[i].capability != want[i].1 {
            ok = false;
        }
        i += 1;
    }
    let orch_ok = ORCHESTRATION.contains("统一编排层") && ORCHESTRATION.contains("各域保留专业降级逻辑");
    if ok && orch_ok {
        set.ok("E241-统一编排-01-历史汇总五行");
    } else {
        set.fail("E241-统一编排-01-历史汇总五行", "汇总行漂移或统一化声明缺失");
    }
    // 统一编排-02 收编核验双向：在表内过、表外孤儿立案。
    if legacy_enrolled("J03", "热阶梯").is_ok()
        && legacy_enrolled("N01", "效果降级").is_ok()
        && legacy_enrolled("X99", "不存在的能力") == Err(C_VCO01_LEGACY_ORPHAN)
    {
        set.ok("E241-统一编排-02-收编双向闸");
    } else {
        set.fail("E241-统一编排-02-收编双向闸", "表外能力未被立案或在表能力被误拒");
    }
}

fn chk_invariants(set: &mut CheckSet) {
    // 不变量-01 双不变量字面量 + 底线闸恰端点行为：79/80 拒、80/80 过、
    // 完整性 false 拒 true 过（判据「不变量」）。
    let inv_ok = INVARIANTS[0].contains("80 帧适配表")
        && INVARIANTS[0].contains("合同底线")
        && INVARIANTS[1].contains("信息完整性");
    let floor_ok = floor_check(79, 80) == Err(C_VCO01_FLOOR_BROKEN)
        && floor_check(80, 80).is_ok()
        && floor_check(81, 80).is_ok();
    let integ_ok = integrity_check(false) == Err(C_VCO01_INTEGRITY_LOST)
        && integrity_check(true).is_ok();
    if inv_ok && floor_ok && integ_ok {
        set.ok("E241-不变量-01-双底线闸");
    } else {
        set.fail("E241-不变量-01-双底线闸", "底线被突破未拒或字面量漂移");
    }
    // 风险-01 四条风险预案互异非空：名称独立对拍 + 预案含「预案」且互不相同。
    let want_names = ["决策振荡", "链路冲突", "恢复过冲", "可见性失控"];
    let mut n_ok = RISKS.len() == 4;
    let mut i = 0usize;
    while n_ok && i < 4 {
        if RISKS[i].0 != want_names[i] || RISKS[i].1.is_empty() || !RISKS[i].1.contains("预案") {
            n_ok = false;
        }
        let mut j = i + 1;
        while j < 4 {
            if RISKS[i].1 == RISKS[j].1 {
                n_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if n_ok {
        set.ok("E241-风险-01-四条配预案");
    } else {
        set.fail("E241-风险-01-四条配预案", "风险漂移或预案缺失/雷同");
    }
    // 摘要-01 摘要承载：版本 + 域段 + 统一编排原句。
    let line = api::screen_line();
    if line.contains(VCO01_VERSION)
        && line.contains("2241-2400")
        && line.contains("触发→决策→执行→恢复→观测")
        && line.contains(ORCHESTRATION)
    {
        set.ok("E241-摘要-01-摘要承载");
    } else {
        set.fail("E241-摘要-01-摘要承载", "版本/域段/声明漂移");
    }
}

// ---------------------------------------------------------------------------
// B 族：判据承载力
// ---------------------------------------------------------------------------

fn chk_codes(set: &mut CheckSet) {
    // 判据-01 六码独占 0x55xx 段、互异、与 0x50-0x54 邻段零重叠。
    let codes = [
        C_VCO01_FLOOR_BROKEN,
        C_VCO01_INTEGRITY_LOST,
        C_VCO01_RECEIPT_INVALID,
        C_VCO01_STAGE_SKIP,
        C_VCO01_STAGE_REWIND,
        C_VCO01_LEGACY_ORPHAN,
    ];
    let mut distinct = true;
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    let seg_ok = codes.iter().all(|c| (c >> 8) == 0x55);
    let no_clash = codes.iter().all(|c| *c > 0x54FF);
    if distinct && seg_ok && no_clash {
        set.ok("E241-判据-01-码段独占");
    } else {
        set.fail("E241-判据-01-码段独占", "码漂移或撞段");
    }
}

/// 判据侧手写 FNV 混合（双源同构——不复用 fnv64 代码路径）。
fn ref_fp(items: &[&str]) -> u64 {
    let mut acc: u64 = 0x2237_2237_2237_2237;
    let mut i = 0usize;
    while i < items.len() {
        let b = items[i].as_bytes();
        let mut j = 0usize;
        while j < b.len() {
            acc ^= b[j] as u64;
            acc = acc.wrapping_mul(0x0000_0100_0000_01b3);
            j += 1;
        }
        i += 1;
    }
    acc
}

fn chk_fingerprint(set: &mut CheckSet) {
    // 判据-02 签收指纹双源对账：判据侧手写混合与 handoff_fp 同值且非零。
    let expect = ref_fp(&N_HANDOFF_ITEMS);
    if expect == N_HANDOFF_FP && expect != 0 && N_HANDOFF.fingerprint == expect {
        set.ok("E241-判据-02-签收指纹双源");
    } else {
        set.fail("E241-判据-02-签收指纹双源", "指纹现算失配或种子漂移");
    }
}

/// 单遍词法剥除（字符串/注释不误伤）。
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

fn chk_zero_panic(set: &mut CheckSet) {
    // 判据-03 生产面零 panic。
    let src = include_str!("vco01_degrade.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E241-判据-03-生产面零 panic");
    } else {
        set.fail("E241-判据-03-生产面零 panic", "生产面含 panic 面");
    }
    // 判据-04 判据面零 panic（自扫）。
    let src = include_str!("vco01_degrade_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E241-判据-04-判据面零 panic");
    } else {
        set.fail("E241-判据-04-判据面零 panic", "判据面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-05 聚合守恒防自调。
    let a = run_vco01_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E241-判据-05-聚合守恒防自调");
    } else {
        set.fail("E241-判据-05-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 判据族 a：使命 + 签收 + 映射 + 五段 + 统一编排 + 不变量 + 风险。
pub fn run_vco01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/vco01/a");
    chk_mission(&mut s);
    chk_mapping(&mut s);
    chk_stages(&mut s);
    chk_orchestration(&mut s);
    chk_invariants(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vco01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("cgpu/vco01/b");
    chk_codes(&mut s);
    chk_fingerprint(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vco01_checks() -> CheckSet {
    CheckSet::merge(run_vco01_checks_a_standalone(), run_vco01_checks_b_standalone())
}
