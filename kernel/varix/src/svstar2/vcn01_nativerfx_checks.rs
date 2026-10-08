//! CGPU-F2081 域自检（CGPU-N 域开工判据层）
//!
//! 判据侧**独立写死**期望（七件名册、四主题、十组连续、五段秩、双门禁阈值
//! 98/2、0x9C 码段）。聚合防自调：族内判据只调另一族 standalone + 进行中
//! set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcn01_nativerfx::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_CODES: [u16; 6] = [
    0x9C00, 0x9C01, 0x9C02, 0x9C03, 0x9C04, 0x9C05,
];
const EXPECT_SEVEN: usize = 7;
const EXPECT_THEMES: usize = 4;
const EXPECT_GROUPS: usize = 10;
const EXPECT_STAGES: usize = 5;
const EXPECT_DOMAIN_SIZE: usize = 160;
const EXPECT_SSIM: u32 = 98;
const EXPECT_LATENCY: u32 = 2;

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
// 一、规格（使命 / 签收 / 映射 / 五段 / 来源 / 双门禁 / 风险）
// ---------------------------------------------------------------------------

fn chk_spec_mission(set: &mut CheckSet) {
    // 规格-01：等价声明可 grep——「像素级重建」「不是模仿，是等价」独立 contains。
    let ok = MISSION.contains("像素级重建")
        && MISSION.contains("不是模仿，是等价")
        && MISSION.contains("Acrylic")
        && MISSION.contains("毛玻璃");
    if ok {
        set.ok("EN081-规格-01-等价声明判据词在句");
    } else {
        set.fail("EN081-规格-01-等价声明判据词在句", "使命漂移或判据词缺失");
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
        set.ok("EN081-规格-02-七件全签兑付");
    } else {
        set.fail("EN081-规格-02-七件全签兑付", "全签未兑付或计数漂移");
    }
}

fn chk_spec_handoff_edges(set: &mut CheckSet) {
    // 边界-01：重复签与越界双向显性拒。
    let mut led = HandoffLedger::new();
    let first = led.sign(0);
    let dup = led.sign(0);
    let oor = led.sign(7);
    let ok = first.is_ok()
        && dup == Err(E_N081_DUP_SIGN)
        && oor == Err(E_N081_SIGN_RANGE)
        && led.sign_actions == 1;
    if ok {
        set.ok("EN081-边界-01-重复签越界双向拒");
    } else {
        set.fail("EN081-边界-01-重复签越界双向拒", "签收闸未两侧钉死");
    }
    // 边界-02：缺件不兑付——只签六件 ready 必假。
    let mut led = HandoffLedger::new();
    let mut i = 0usize;
    while i < EXPECT_SEVEN - 1 {
        let _ = led.sign(i);
        i += 1;
    }
    let ok = led.signed_count() == 6 && !led.ready_to_extend();
    if ok {
        set.ok("EN081-边界-02-缺件不兑付");
    } else {
        set.fail("EN081-边界-02-缺件不兑付", "六件即兑付=签收闸被架空");
    }
}

fn chk_spec_groups_stages(set: &mut CheckSet) {
    // 规格-03：四主题封闭（判据侧写死首尾主题名）。
    let ok = THEMES.len() == EXPECT_THEMES
        && THEMES[0] == "模糊景深"
        && THEMES[3] == "阴影重建"
        && GROUPS.len() == EXPECT_GROUPS
        && group_themes_valid();
    if ok {
        set.ok("EN081-规格-03-主题封闭组承接合法");
    } else {
        set.fail("EN081-规格-03-主题封闭组承接合法", "主题/组映射漂移");
    }
    // 规格-04：十组连续 + 域段 160 单守恒（判据侧独立重算 2240-2081+1）。
    let recount = 2240usize - 2081usize + 1;
    let ok = groups_contiguous()
        && domain_size() == EXPECT_DOMAIN_SIZE
        && recount == EXPECT_DOMAIN_SIZE;
    if ok {
        set.ok("EN081-规格-04-组连续域段守恒");
    } else {
        set.fail("EN081-规格-04-组连续域段守恒", "组断洞或域段失守");
    }
    // 规格-05：五段顺序秩 + 越界拒（第六段不存在）。
    let s0 = stage_of(0);
    let s4 = stage_of(4);
    let oor = stage_of(5);
    let ok = s0 == Ok("效果清单")
        && s4 == Ok("性能治理")
        && oor == Err(E_N081_STAGE_RANGE)
        && ARCH_STAGES.len() == EXPECT_STAGES;
    if ok {
        set.ok("EN081-规格-05-五段秩与越界拒");
    } else {
        set.fail("EN081-规格-05-五段秩与越界拒", "段序漂移或越界未拒");
    }
}

fn chk_spec_gate(set: &mut CheckSet) {
    // 规格-06：O 域来源契约逐句可 grep（backdrop/filter/跨域契约/不私设白名单）。
    let ok = O_RELATION.contains("backdrop")
        && O_RELATION.contains("filter")
        && O_RELATION.contains("跨域契约")
        && O_RELATION.contains("白名单");
    if ok {
        set.ok("EN081-规格-06-来源契约逐句在案");
    } else {
        set.fail("EN081-规格-06-来源契约逐句在案", "来源协议漂移");
    }
    // 规格-07：双门禁四象限恰边界——SSIM 98 恰过 97 拒、延迟 2 恰过 3 拒
    //（<⇄<= 与 >⇄>= 变异两侧钉死）。
    let pass = equivalence_gate(98, 2);
    let ssim_edge = equivalence_gate(97, 2);
    let lat_edge = equivalence_gate(98, 3);
    let both_bad = equivalence_gate(0, 255);
    let ok = pass == Equivalence::Equivalent
        && ssim_edge == Equivalence::PixelMismatch
        && lat_edge == Equivalence::TimingMismatch
        && both_bad == Equivalence::PixelMismatch
        && EXPECT_SSIM == 98
        && EXPECT_LATENCY == 2;
    if ok {
        set.ok("EN081-规格-07-双门禁四象限恰边界");
    } else {
        set.fail("EN081-规格-07-双门禁四象限恰边界", "等价判定被架空或阈值漂移");
    }
    // 规格-08：风险四条各配非空预案。
    let mut ok = RISKS.len() == 4;
    let mut i = 0usize;
    while i < RISKS.len() {
        if RISKS[i].0.is_empty() || RISKS[i].1.is_empty() {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EN081-规格-08-风险预案齐备");
    } else {
        set.fail("EN081-规格-08-风险预案齐备", "风险缺预案");
    }
}

// ---------------------------------------------------------------------------
// 二、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vcn01_nativerfx_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EN081-判据-01-判据面零 panic");
    } else {
        set.fail("EN081-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-09：生产面零 panic（扫实现文件）。
    let src = include_str!("vcn01_nativerfx.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EN081-规格-09-生产面零 panic");
    } else {
        set.fail("EN081-规格-09-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——六码皆 0x9C 细分段、互异、与写死值逐位等。
    let got = [
        E_N081_SIGN_RANGE,
        E_N081_DUP_SIGN,
        E_N081_SSIM_LOW,
        E_N081_LATENCY_HIGH,
        E_N081_STAGE_RANGE,
        E_N081_THEME_RANGE,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x9C00 || got[i] != EXPECT_CODES[i] {
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
        set.ok("EN081-判据-02-码段独占独立复核");
    } else {
        set.fail("EN081-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 10 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vcn01_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 10;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EN081-判据-03-聚合守恒防自调");
    } else {
        set.fail("EN081-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界。
pub fn run_vcn01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcn01/a");
    chk_spec_mission(&mut s);
    chk_spec_handoff_edges(&mut s);
    chk_spec_groups_stages(&mut s);
    chk_spec_gate(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vcn01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcn01/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcn01_checks() -> CheckSet {
    CheckSet::merge(run_vcn01_checks_a_standalone(), run_vcn01_checks_b_standalone())
}
