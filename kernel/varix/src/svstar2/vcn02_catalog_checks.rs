//! CGPU-F2082 域自检（CGPU-N 域效果清单判据层）
//!
//! 判据侧**独立写死**期望（四族八条、P0 快照、版本号、0x9C1x 码段与 vcn01
//! 六码合并互异）。聚合防自调：族内判据只调另一族 standalone + 进行中 set
//! 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcn02_catalog::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_VERSION: &str = "NC02-catalog-v1";
const EXPECT_ENTRIES: usize = 8;

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
// 一、规格（四族 / 清单 / 优先级 / 来源 / 版本化）
// ---------------------------------------------------------------------------

fn chk_spec_catalog(set: &mut CheckSet) {
    // 规格-01：四族封闭往返 + 越界 None。
    let mut rt = true;
    let mut i = 0usize;
    while i < 4 {
        match EffectFamily::of_ordinal(i) {
            Some(f) => {
                if f.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    let oor = EffectFamily::of_ordinal(4).is_none();
    if rt && oor && EffectFamily::ALL.len() == 4 {
        set.ok("EN082-规格-01-四族封闭往返");
    } else {
        set.fail("EN082-规格-01-四族封闭往返", "族漂移或越界未拒");
    }
    // 规格-02：清单封闭八条 + 条目查询恰边界（第 8 条 Ok、第 9 条拒）。
    let last = entry_of(7);
    let oor = entry_of(8);
    let ok = CATALOG.len() == EXPECT_ENTRIES
        && last == Ok(CATALOG[7])
        && oor == Err(E_N082_ENTRY_RANGE);
    if ok {
        set.ok("EN082-规格-02-清单八条恰边界");
    } else {
        set.fail("EN082-规格-02-清单八条恰边界", "条目数漂移或越界未拒");
    }
    // 规格-03：四族各两条守恒 + 族内名查询判据侧写死对拍（首尾两族）。
    let blur = names_of_family(EffectFamily::Blur);
    let shape = names_of_family(EffectFamily::Shape);
    let ok = family_balanced()
        && blur[0] == "Acrylic"
        && blur[1] == "背景模糊"
        && shape[0] == "圆角"
        && shape[1] == "阴影";
    if ok {
        set.ok("EN082-规格-03-族守恒与名册对拍");
    } else {
        set.fail("EN082-规格-03-族守恒与名册对拍", "族平衡破坏或名册漂移");
    }
}

fn chk_spec_priority_source(set: &mut CheckSet) {
    // 规格-04：优先级封闭秩序 P0<P1<P2 + P0 快照判据侧写死对拍。
    let ord_ok = Priority::P0 < Priority::P1 && Priority::P1 < Priority::P2;
    let p0 = p0_names();
    let ok = ord_ok && p0[0] == "Acrylic" && p0[1] == "背景模糊" && p0[2] == "Mica";
    if ok {
        set.ok("EN082-规格-04-优先级秩序与P0快照");
    } else {
        set.fail("EN082-规格-04-优先级秩序与P0快照", "优先级漂移或快照失配");
    }
    // 规格-05：来源逐条在册（判据侧独立写死三条对拍——Acrylic=Win32、
    // Mica=Dwm、背景模糊=Browser）。
    let acrylic = CATALOG[0];
    let mica = CATALOG[2];
    let bblur = CATALOG[1];
    let ok = acrylic.name == "Acrylic"
        && acrylic.source == EffectSource::Win32
        && mica.name == "Mica"
        && mica.source == EffectSource::Dwm
        && bblur.name == "背景模糊"
        && bblur.source == EffectSource::Browser;
    if ok {
        set.ok("EN082-规格-05-来源标注逐条在册");
    } else {
        set.fail("EN082-规格-05-来源标注逐条在册", "来源标注漂移");
    }
    // 规格-06：版本化——版本常量判据侧写死等 + 对账恰等过/失配拒双向。
    let pass = check_version("NC02-catalog-v1");
    let stale = check_version("NC02-catalog-v0");
    let ok = CATALOG_VERSION == EXPECT_VERSION
        && pass.is_ok()
        && stale == Err(E_N082_VERSION_STALE);
    if ok {
        set.ok("EN082-规格-06-版本化对账双向");
    } else {
        set.fail("EN082-规格-06-版本化对账双向", "版本闸未两侧钉死");
    }
}

// ---------------------------------------------------------------------------
// 二、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vcn02_catalog_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EN082-判据-01-判据面零 panic");
    } else {
        set.fail("EN082-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-07：生产面零 panic（扫实现文件）。
    let src = include_str!("vcn02_catalog.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EN082-规格-07-生产面零 panic");
    } else {
        set.fail("EN082-规格-07-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——本域三码皆 0x9C1x；与 vcn01 六码（0x9C0x）
    // 合并九码互异（同段跨文件防撞码）。
    let got = [E_N082_VERSION_STALE, E_N082_ENTRY_RANGE, E_N082_FAMILY_RANGE];
    let vcn01_codes = [0x9C00u16, 0x9C01, 0x9C02, 0x9C03, 0x9C04, 0x9C05];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x9C00 || got[i] & 0x00F0 != 0x0010 {
            ok = false;
        }
        let mut j = 0usize;
        while j < vcn01_codes.len() {
            if got[i] == vcn01_codes[j] {
                ok = false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < got.len() {
            if got[i] == got[k] {
                ok = false;
            }
            k += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("EN082-判据-02-码段独占独立复核");
    } else {
        set.fail("EN082-判据-02-码段独占独立复核", "码漂移或与 vcn01 撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 6 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vcn02_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 6;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EN082-判据-03-聚合守恒防自调");
    } else {
        set.fail("EN082-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格。
pub fn run_vcn02_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcn02/a");
    chk_spec_catalog(&mut s);
    chk_spec_priority_source(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vcn02_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcn02/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcn02_checks() -> CheckSet {
    CheckSet::merge(run_vcn02_checks_a_standalone(), run_vcn02_checks_b_standalone())
}
