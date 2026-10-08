//! CGPU-F2242 域自检（CGPU-O 域降级维度统一模型判据层）
//!
//! 判据侧**独立写死**期望（六维名册、schema 版本号、64 组合、映射条目、
//! 0x9D 码段与 0x9C 全段十三码合并互异）。聚合防自调：族内判据只调另一族
//! standalone + 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vco42_dimmodel::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_VERSION: &str = "OC42-dimschema-v1";
const EXPECT_COMBOS: usize = 64;
const EXPECT_MAPPINGS: usize = 6;

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
// 一、规格（六维 / schema / 正交 / 映射——锚点四组）
// ---------------------------------------------------------------------------

fn chk_spec_dims_schema(set: &mut CheckSet) {
    // 规格-01：六维封闭往返 + 越界 None。
    let mut rt = true;
    let mut i = 0usize;
    while i < 6 {
        match Dims::of_ordinal(i) {
            Some(d) => {
                if d.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    let oor = Dims::of_ordinal(6).is_none();
    if rt && oor && Dims::ALL.len() == 6 {
        set.ok("EO082-规格-01-六维封闭往返");
    } else {
        set.fail("EO082-规格-01-六维封闭往返", "维度漂移或越界未拒");
    }
    // 规格-02：schema 六条齐——三元组字段非空（档位/触发/恢复缺一不可）
    // 且档位数 ≥2（单档无降级空间=伪维度）。
    let mut ok = DIM_SCHEMAS.len() == 6;
    let mut i = 0usize;
    while i < DIM_SCHEMAS.len() {
        let s = DIM_SCHEMAS[i];
        if s.trigger.is_empty() || s.recover.is_empty() || s.levels < 2 {
            ok = false;
        }
        if s.dim.ordinal() != i {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EO082-规格-02-三元组齐备");
    } else {
        set.fail("EO082-规格-02-三元组齐备", "schema 漏字段或伪维度");
    }
    // 规格-03：schema 版本化对账双向（恰等过/失配拒）+ 判据侧写死版本号。
    let pass = check_schema_version("OC42-dimschema-v1");
    let stale = check_schema_version("OC42-dimschema-v0");
    let ok = SCHEMA_VERSION == EXPECT_VERSION
        && pass.is_ok()
        && stale == Err(E_O082_SCHEMA_STALE);
    if ok {
        set.ok("EO082-规格-03-schema版本对账双向");
    } else {
        set.fail("EO082-规格-03-schema版本对账双向", "版本闸未两侧钉死");
    }
}

fn chk_spec_ortho_mapping(set: &mut CheckSet) {
    // 规格-04：正交声明可 grep（独立/可组合/互不阻塞三词）+ 组合闸双向
    //（零组合拒、越六位拒、单维与满六维合法）。
    let grep_ok = DIMS_ORTHOGONAL.contains("独立")
        && DIMS_ORTHOGONAL.contains("可组合")
        && DIMS_ORTHOGONAL.contains("互不阻塞");
    let zero = combo_legal(0);
    let over = combo_legal(1u8 << 6);
    let single = combo_legal(1);
    let full = combo_legal(0x3F);
    let ok = grep_ok
        && zero == Err(E_O082_COMBO_ILLEGAL)
        && over == Err(E_O082_COMBO_ILLEGAL)
        && single.is_ok()
        && full.is_ok();
    if ok {
        set.ok("EO082-规格-04-正交声明组合闸");
    } else {
        set.fail("EO082-规格-04-正交声明组合闸", "正交失守或组合闸未钉死");
    }
    // 规格-05：组合空间守恒——2^6=64 判据侧独立重算（移位展开六次）。
    let recount = {
        let mut n = 1usize;
        let mut i = 0usize;
        while i < 6 {
            n *= 2;
            i += 1;
        }
        n
    };
    let ok = orthogonal_combos() == EXPECT_COMBOS && recount == EXPECT_COMBOS;
    if ok {
        set.ok("EO082-规格-05-组合空间64守恒");
    } else {
        set.fail("EO082-规格-05-组合空间64守恒", "组合空间漂移");
    }
    // 规格-06：映射表封闭六条 + 判据侧写死对拍（C→Quality、D→FrameRate、
    // N→Resolution）+ 缺源拒。
    let c = mapping_of("C 域降质链（CGPU-F0008）");
    let d = mapping_of("D 域降帧事故（CGPU-F0482）");
    let n = mapping_of("N 域重建降级（CGPU-F2081）");
    let unknown = mapping_of("不存在域");
    let ok = DOMAIN_MAPPINGS.len() == EXPECT_MAPPINGS
        && c == Ok(Dims::Quality)
        && d == Ok(Dims::FrameRate)
        && n == Ok(Dims::Resolution)
        && unknown == Err(E_O082_DOMAIN_UNKNOWN)
        && MAPPING_FAMILY.contains("映射即复用");
    if ok {
        set.ok("EO082-规格-06-映射表封闭与复用声明");
    } else {
        set.fail("EO082-规格-06-映射表封闭与复用声明", "映射漂移或缺源未拒");
    }
}

// ---------------------------------------------------------------------------
// 二、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vco42_dimmodel_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EO082-判据-01-判据面零 panic");
    } else {
        set.fail("EO082-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-07：生产面零 panic（扫实现文件）。
    let src = include_str!("vco42_dimmodel.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EO082-规格-07-生产面零 panic");
    } else {
        set.fail("EO082-规格-07-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——本域四码皆 0x9D；与 N 域 vcn01/02/03
    // 十三码（0x9C 全段）合并互异（跨域段防撞码）。
    let got = [E_O082_SCHEMA_STALE, E_O082_DIM_RANGE, E_O082_COMBO_ILLEGAL, E_O082_DOMAIN_UNKNOWN];
    let prior = [
        0x9C00u16, 0x9C01, 0x9C02, 0x9C03, 0x9C04, 0x9C05,
        0x9C10, 0x9C11, 0x9C12, 0x9C20, 0x9C21, 0x9C22, 0x9C23,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x9D00 {
            ok = false;
        }
        let mut j = 0usize;
        while j < prior.len() {
            if got[i] == prior[j] {
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
        set.ok("EO082-判据-02-码段独占独立复核");
    } else {
        set.fail("EO082-判据-02-码段独占独立复核", "码漂移或与 N 域撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 6 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vco42_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 6;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EO082-判据-03-聚合守恒防自调");
    } else {
        set.fail("EO082-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格。
pub fn run_vco42_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vco42/a");
    chk_spec_dims_schema(&mut s);
    chk_spec_ortho_mapping(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vco42_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vco42/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vco42_checks() -> CheckSet {
    CheckSet::merge(run_vco42_checks_a_standalone(), run_vco42_checks_b_standalone())
}
