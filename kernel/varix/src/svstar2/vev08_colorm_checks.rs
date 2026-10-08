//! VE-F4408 域自检（VE-V 域色彩与 M 域深化判据层）
//!
//! 判据侧**独立写死**期望（SDR 缺省值、三步协议语义、单次变换口径、
//! 分离语料）。聚合防自调：族内判据只调另一族 standalone + 进行中 set
//! 自身 tally，守恒断言归 CI 探针层。锚点判据四条 → 映射：元数据透传
//! （规格-01/02）、三步协商（规格-03/04）、单次变换（规格-05/06）、
//! SDR 回退（规格-03/04 内嵌）。

use crate::checks::CheckSet;

use super::vev08_colorm::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

/// 判据侧独立写死的 SDR 缺省（与实现常量对拍）。
const EXPECT_SDR: ColorMeta = ColorMeta { primaries: 1, transfer: 1, matrix: 1, range: 2 };

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
// 一、规格（元数据透传 / 三步协商 / 单次变换 / SDR 回退——锚点四判据）
// ---------------------------------------------------------------------------

fn chk_spec_passthrough(set: &mut CheckSet) {
    // 规格-01：元数据透传——在则原样直达（四字段逐位相等），丢失则缺省标注
    // 不臆造（is_default=true 且值 = 判据侧写死 SDR 缺省）。
    let src = ColorMeta { primaries: 9, transfer: 3, matrix: 2, range: 1 };
    let pass = passthrough(Some(src));
    let miss = passthrough(None);
    let ok = pass.meta == src
        && !pass.is_default
        && miss.is_default
        && miss.meta == EXPECT_SDR
        && SDR_DEFAULT == EXPECT_SDR;
    if ok {
        set.ok("EV08-规格-01-透传与缺省标注");
    } else {
        set.fail("EV08-规格-01-透传与缺省标注", "透传丢字段或缺省臆造");
    }
    // 规格-02：缺省语义可测——缺省回执可被下游识别（is_default 与值分离，
    // 且缺省值 ≠ 任意真实元数据都标非缺省：非缺省路径 is_default 恒假）。
    let sdr_real = ColorMeta { primaries: 1, transfer: 1, matrix: 1, range: 2 };
    let real = passthrough(Some(sdr_real));
    let ok = real.meta == EXPECT_SDR && !real.is_default;
    if ok {
        set.ok("EV08-规格-02-缺省与真实SDR可分辨");
    } else {
        set.fail("EV08-规格-02-缺省与真实SDR可分辨", "缺省标注被架空");
    }
}

fn chk_spec_negotiate(set: &mut CheckSet) {
    // 规格-03：三步协商全路径——意图可达原样回执；不可达接受回退 →
    // SDR 回退回执（fell_back_sdr=true 且理由非空）；不可达不接受回退 →
    // 显性失败（None——降级矩阵阻断而非静默改道）。
    let caps_hdr = DisplayCaps { sdr: true, hdr10: true, hlg: false };
    let direct = negotiate(Intent { target: ColorSpace::Hdr10, allow_fallback: true }, caps_hdr);
    let fallback = negotiate(
        Intent { target: ColorSpace::Hlg, allow_fallback: true },
        caps_hdr,
    );
    let fail = negotiate(
        Intent { target: ColorSpace::Hlg, allow_fallback: false },
        caps_hdr,
    );
    let ok = match direct {
        Some(r) => r.space == ColorSpace::Hdr10 && !r.fell_back_sdr && !r.reason.is_empty(),
        None => false,
    } && match fallback {
        Some(r) => r.space == ColorSpace::Sdr && r.fell_back_sdr,
        None => false,
    } && fail.is_none();
    if ok {
        set.ok("EV08-规格-03-三步协商全路径");
    } else {
        set.fail("EV08-规格-03-三步协商全路径", "协商回执失真或失败未显性");
    }
    // 规格-04：协商确定性——同意图同能力两次裁决同回执（O(1) 无状态）。
    let i1 = negotiate(Intent { target: ColorSpace::Sdr, allow_fallback: true }, caps_hdr);
    let i2 = negotiate(Intent { target: ColorSpace::Sdr, allow_fallback: true }, caps_hdr);
    if i1.is_some() && i1 == i2 {
        set.ok("EV08-规格-04-协商确定性");
    } else {
        set.fail("EV08-规格-04-协商确定性", "同输入不同回执");
    }
}

fn chk_spec_single_transform(set: &mut CheckSet) {
    // 规格-05：单次变换断言——零级/一级过；两级检出且计数恰 2；三级恰 3
    //（O(链长) 扫描与链长无关结论只看变换级数）。
    let mk = |name: &'static str, converts: bool| Stage { name, converts };
    let zero = [mk("src", false), mk("compose", false)];
    let one = [mk("src", false), mk("csc", true), mk("blend", false)];
    let two = [mk("csc1", true), mk("mid", false), mk("csc2", true)];
    let three = [mk("a", true), mk("b", true), mk("c", true)];
    let ok = assert_single_transform(&zero) == ChainAudit::SingleTransform
        && assert_single_transform(&one) == ChainAudit::SingleTransform
        && assert_single_transform(&two) == ChainAudit::DoubleTransform(2)
        && assert_single_transform(&three) == ChainAudit::DoubleTransform(3);
    if ok {
        set.ok("EV08-规格-05-单次变换恰边界");
    } else {
        set.fail("EV08-规格-05-单次变换恰边界", "断言口径漂移");
    }
    // 规格-06：双重变换阻断修正语义——检出后修正链（删多余变换级）再断言过
    // （阻断不是终点，修正可验证）。
    let two = [mk("csc1", true), mk("csc2", true)];
    let fixed = [mk("csc1", true), mk("mid", false)];
    let ok = assert_single_transform(&two) != ChainAudit::SingleTransform
        && assert_single_transform(&fixed) == ChainAudit::SingleTransform;
    if ok {
        set.ok("EV08-规格-06-检出后修正可验证");
    } else {
        set.fail("EV08-规格-06-检出后修正可验证", "阻断修正闭环失守");
    }
}

// ---------------------------------------------------------------------------
// 二、判据承载力（零 panic / 防自调）
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vev08_colorm_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EV08-判据-01-判据面零 panic");
    } else {
        set.fail("EV08-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-07：生产面零 panic（扫实现文件）。
    let src = include_str!("vev08_colorm.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EV08-规格-07-生产面零 panic");
    } else {
        set.fail("EV08-规格-07-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-02：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 6 条；判据-02 登记前 B 族进行中 2 条）。
    let a = run_vev08_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 6;
    let self_ok = sp + sf == 2;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EV08-判据-02-聚合守恒防自调");
    } else {
        set.fail("EV08-判据-02-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格。
pub fn run_vev08_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vev08/a");
    chk_spec_passthrough(&mut s);
    chk_spec_negotiate(&mut s);
    chk_spec_single_transform(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vev08_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vev08/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vev08_checks() -> CheckSet {
    CheckSet::merge(run_vev08_checks_a_standalone(), run_vev08_checks_b_standalone())
}
