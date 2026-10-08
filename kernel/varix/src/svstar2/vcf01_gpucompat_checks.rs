//! CGPU-F0801 域自检（判据逐条映射：六主题/铁律兑现/十组规划/D08 分工）
//!
//! 判据侧独立重算主题计数、组号连续性、量化手算值——同源恒绿的弱门禁比没有
//! 门禁更坏。聚合防自调：A/B 两族 + tally 守恒（F2806/F0817 首版无限递归教训）。

use crate::checks::CheckSet;

use super::vcf01_gpucompat as api;
use super::vcf01_gpucompat::*;

use alloc::string::String;

/// A 族条数（判据侧写死）。
const EXPECT_A_COUNT: usize = 13;
/// B 族登记本条前条数（判据侧写死）。
const EXPECT_B_BEFORE: usize = 3;

fn chk_topics(set: &mut CheckSet) {
    // 规格-01 六主题封闭：数恒 6、号 1..=6 连续、段含端点落在域段内。
    let mut no_ok = true;
    let mut expect = 1u8;
    for t in TOPICS.iter() {
        if t.no != expect {
            no_ok = false;
        }
        expect += 1;
        if t.range.0 > t.range.1 || t.range.0 < DOMAIN_RANGE.0 || t.range.1 > DOMAIN_RANGE.1 {
            no_ok = false;
        }
    }
    if TOPICS.len() == 6 && no_ok && expect == 7 {
        set.ok("E801-规格-01-六主题封闭");
    } else {
        set.fail("E801-规格-01-六主题封闭", "主题计数/号段/任务段漂移");
    }
    // 规格-02 十组规划：组号 F01..F10 连续无洞（判据侧独立重算）。
    if GROUPS.len() == 10 && groups_contiguous() {
        set.ok("E801-规格-02-十组连续");
    } else {
        set.fail("E801-规格-02-十组连续", "组号断洞或计数漂移");
    }
    // 规格-03 组-主题映射合法：主题号 1..=6，仅 F01/F10 允许空段 0。
    if group_themes_valid() {
        set.ok("E801-规格-03-组主题映射");
    } else {
        set.fail("E801-规格-03-组主题映射", "映射越界或空段违规");
    }
    // 规格-04 铁律 8 原句承载：方法论常量与摘要行同文。
    let line = screen_line();
    if METHODOLOGY.contains("实测认证不凭文档声明")
        && METHODOLOGY.contains("实机测试认证")
        && line.contains(METHODOLOGY)
        && line.contains("topics=6")
        && line.contains("groups=10")
    {
        set.ok("E801-规格-04-铁律原句承载");
    } else {
        set.fail("E801-规格-04-铁律原句承载", "方法论声明或摘要漂移");
    }
}

fn chk_methodology(set: &mut CheckSet) {
    // 铁律-01 文档声明进不了认证：doc_claim_only → 0x9F00 拒绝。
    let doc_only = CertEvidence {
        device: 0x8086_0046,
        driver: 101,
        suite_run: true,      // 即便声称测过
        doc_claim_only: true, // 但只有文档声明
    };
    if certify(&doc_only) == Err(C_VCF01_UNVERIFIED) {
        set.ok("E801-铁律-01-文档声明拒认证");
    } else {
        set.fail("E801-铁律-01-文档声明拒认证", "文档声明被放行（铁律失守）");
    }
    // 铁律-02 未实测 → 显性 Uncertified（没测过就说没测过，不静默也不拒绝）。
    let untested = CertEvidence { device: 1, driver: 1, suite_run: false, doc_claim_only: false };
    if certify(&untested) == Ok(CertStatus::Uncertified) {
        set.ok("E801-铁律-02-未实测显性状态");
    } else {
        set.fail("E801-铁律-02-未实测显性状态", "未实测被隐式处理");
    }
    // 铁律-03 实机凭据齐全 → Certified。
    let solid = CertEvidence { device: 1, driver: 1, suite_run: true, doc_claim_only: false };
    if certify(&solid) == Ok(CertStatus::Certified) {
        set.ok("E801-铁律-03-实机认证通过");
    } else {
        set.fail("E801-铁律-03-实机认证通过", "实机凭据未被认证");
    }
}

fn chk_d08(set: &mut CheckSet) {
    // 分工-01 契约核验：职责互斥 + 接口点齐备。
    if CONTRACT.verify().is_ok() {
        set.ok("E801-分工-01-契约互斥核验");
    } else {
        set.fail("E801-分工-01-契约互斥核验", "合法契约被拒");
    }
    // 分工-02 越界可观测：职责重叠 → 0x9F02。
    let bad = DivisionContract {
        d08_duties: ["出考卷", "判定"],
        f_duties: ["判定", "成绩"], // 判定越界进本域
        handoffs: ["考生清单交考场", "成绩单回矩阵"],
    };
    if bad.verify() == Err(C_VCF01_D08_OVERLAP) {
        set.ok("E801-分工-02-越界拒绝");
    } else {
        set.fail("E801-分工-02-越界拒绝", "职责重叠未被拒绝");
    }
    // 分工-03 接口点缺失可观测。
    let no_handoff = DivisionContract {
        d08_duties: ["出考卷", "判定"],
        f_duties: ["出考生", "成绩"],
        handoffs: ["", ""],
    };
    if no_handoff.verify() == Err(C_VCF01_D08_OVERLAP) {
        set.ok("E801-分工-03-接口点齐备闸");
    } else {
        set.fail("E801-分工-03-接口点齐备闸", "空接口被放行");
    }
    // 分工-04 分工方向文本承载（D08 出考卷与判定 / 本域出考生与成绩）。
    let ok = CONTRACT.d08_duties == ["出考卷", "判定"] && CONTRACT.f_duties == ["出考生", "成绩"];
    if ok {
        set.ok("E801-分工-04-分工方向承载");
    } else {
        set.fail("E801-分工-04-分工方向承载", "分工方向文本漂移");
    }
}

fn chk_planning(set: &mut CheckSet) {
    // 规划-01 任务段：域段 801-960 恰 160 单、六主题段并集不越域段。
    let span = DOMAIN_RANGE.1 - DOMAIN_RANGE.0 + 1;
    let topics_cover = TOPICS.iter().all(|t| t.range.1 <= DOMAIN_RANGE.1);
    if DOMAIN_RANGE == (801, 960) && span == 160 && topics_cover {
        set.ok("E801-规划-01-域段守恒");
    } else {
        set.fail("E801-规划-01-域段守恒", "域段或主题段越界");
    }
    // 规划-02 组号独立重算（不用 groups_contiguous，判据侧手写折叠）。
    let mut acc = 0u32;
    for (i, g) in GROUPS.iter().enumerate() {
        if g.gid as usize == i + 1 {
            acc += 1;
        }
    }
    if acc == 10 {
        set.ok("E801-规划-02-组号独立重算");
    } else {
        set.fail("E801-规划-02-组号独立重算", "组号与下标失配");
    }
}

// ---------------------------------------------------------------------------
// B 族：判据承载力
// ---------------------------------------------------------------------------

fn chk_codes(set: &mut CheckSet) {
    // 判据-01 三码独占 0x9F 段、互异。
    let codes = [C_VCF01_UNVERIFIED, C_VCF01_GROUP_GAP, C_VCF01_D08_OVERLAP];
    let distinct = codes[0] != codes[1] && codes[1] != codes[2] && codes[0] != codes[2];
    let seg_ok = codes.iter().all(|c| c & 0xFF00 == 0x9F00);
    if distinct && seg_ok {
        set.ok("E801-判据-01-码段独占");
    } else {
        set.fail("E801-判据-01-码段独占", "码漂移或撞段");
    }
}

/// 单遍词法剥除（字符串/注释不误伤——F2807 实测教训）。
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
    // 判据-02 生产面零 panic。
    let src = include_str!("vcf01_gpucompat.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E801-判据-02-生产面零 panic");
    } else {
        set.fail("E801-判据-02-生产面零 panic", "生产面含 panic 面");
    }
    // 判据-03 判据面零 panic（自扫）。
    let src = include_str!("vcf01_gpucompat_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E801-判据-03-判据面零 panic");
    } else {
        set.fail("E801-判据-03-判据面零 panic", "判据面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-04 聚合守恒防自调。
    let a = run_vcf01_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E801-判据-04-聚合守恒防自调");
    } else {
        set.fail("E801-判据-04-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 铁律 + 分工 + 规划。
pub fn run_vcf01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcf01/a");
    chk_topics(&mut s);
    chk_methodology(&mut s);
    chk_d08(&mut s);
    chk_planning(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vcf01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcf01/b");
    chk_codes(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcf01_checks() -> CheckSet {
    CheckSet::merge(run_vcf01_checks_a_standalone(), run_vcf01_checks_b_standalone())
}
