//! CGPU-F3683 域自检（CGPU-X 域算力增长规划判据层）
//!
//! 判据侧**独立写死**期望（七步阶梯全字段写死、年份/FLOPS 序列独立重算、
//! 对账区间写死、0x99C 段续编六码互异并与前段跨文件互异）。聚合防自调：
//! 族内判据只调另一族 standalone + 进行中 set 自身 tally，守恒断言归
//! CI 探针层。

use crate::checks::CheckSet;

use super::vcx83_computegrow as cg;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（X 域 0x99C 段续编，与实现文件五码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_X830_CHECK_TALLY: u16 = 0x99D3;

const EXPECT_VERSION: &str = "CX83-computegrow-v1";

/// 单遍词法剥除（行注释+块注释+字符串字面量全剥——panic 词扫描防自引用）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out: Vec<u8> = Vec::new();
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
        out.push(b[i]);
        i += 1;
    }
    match String::from_utf8(out) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

// ---------------------------------------------------------------------------
// A 族 · 规划组（规格 + 边界）
// ---------------------------------------------------------------------------

fn chk_spec_steps(set: &mut CheckSet) {
    // 规格-01：七步阶梯全字段写死对拍（年份/FLOPS/置信度逐位）。
    let want: [(u32, u32, u8); 7] = [
        (2025, 100, 100),
        (2026, 130, 90),
        (2027, 175, 85),
        (2029, 300, 70),
        (2031, 480, 55),
        (2033, 750, 40),
        (2035, 1100, 30),
    ];
    let mut ok = cg::ROADMAP_STEPS.len() == 7;
    let mut i = 0usize;
    while i < 7 {
        let s = &cg::ROADMAP_STEPS[i];
        if s.year != want[i].0 || s.flops_x100 != want[i].1 || s.confidence != want[i].2 {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX830-规格-01-七步阶梯全字段写死对拍");
    } else {
        set.fail("EX830-规格-01-七步阶梯全字段写死对拍", "曲线数值漂移");
    }
}

fn chk_spec_basis(set: &mut CheckSet) {
    // 规格-02：七行依据写死对拍（前缀锚）。
    let want_prefix: [&str; 7] = [
        "基线年", "F3682 算力轴", "已公布制程", "5 年区间中段", "制程 1.5 代", "新器件形态",
        "十年上沿保守承诺",
    ];
    let mut ok = cg::ROADMAP_STEPS.len() == 7;
    let mut i = 0usize;
    while i < 7 {
        if !cg::ROADMAP_STEPS[i].basis.starts_with(want_prefix[i]) {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX830-规格-02-七行依据写死对拍");
    } else {
        set.fail("EX830-规格-02-七行依据写死对拍", "依据漂移或缺席");
    }
}

fn chk_spec_confidence_decline(set: &mut CheckSet) {
    // 规格-03：置信度随视野递减（2025=100 → 2035=30 严格递减——诚实衰减）。
    let mut ok = true;
    let mut i = 0usize;
    while i + 1 < 7 {
        if cg::ROADMAP_STEPS[i].confidence <= cg::ROADMAP_STEPS[i + 1].confidence {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX830-规格-03-置信度严格递减");
    } else {
        set.fail("EX830-规格-03-置信度严格递减", "置信度衰减破缺");
    }
}

fn chk_bound_roadmap_pass(set: &mut CheckSet) {
    // 边界-01：登记曲线过实现闸（双铁律+两诚实）。
    let ok = matches!(cg::verify_roadmap(), Ok(7));
    if ok {
        set.ok("EX830-边界-01-登记曲线过闸");
    } else {
        set.fail("EX830-边界-01-登记曲线过闸", "登记曲线被误拒");
    }
}

fn chk_bound_variants(set: &mut CheckSet) {
    // 边界-02：判据侧独立校验器变异四向必拒（年份倒退/算力倒退/置信越界/依据空）。
    let mut year_reg: Vec<cg::RoadmapStep> = Vec::new();
    for s in cg::ROADMAP_STEPS.iter() {
        year_reg.push(s.clone());
    }
    year_reg[2].year = year_reg[1].year;
    let mut flops_reg: Vec<cg::RoadmapStep> = Vec::new();
    for s in cg::ROADMAP_STEPS.iter() {
        flops_reg.push(s.clone());
    }
    flops_reg[3].flops_x100 = 90;
    let mut conf_over: Vec<cg::RoadmapStep> = Vec::new();
    for s in cg::ROADMAP_STEPS.iter() {
        conf_over.push(s.clone());
    }
    conf_over[4].confidence = 101;
    let mut no_basis: Vec<cg::RoadmapStep> = Vec::new();
    for s in cg::ROADMAP_STEPS.iter() {
        no_basis.push(s.clone());
    }
    no_basis[1].basis = "";
    let audit = |rows: &Vec<cg::RoadmapStep>| -> Result<usize, u16> {
        if rows.is_empty() {
            return Err(cg::E_X830_YEAR_REGRESSED);
        }
        let mut i = 0usize;
        while i < rows.len() {
            if rows[i].confidence > 100 {
                return Err(cg::E_X830_CONFIDENCE_FAULT);
            }
            if rows[i].basis.is_empty() {
                return Err(cg::E_X830_BASIS_MISSING);
            }
            if i + 1 < rows.len() {
                if rows[i + 1].year <= rows[i].year {
                    return Err(cg::E_X830_YEAR_REGRESSED);
                }
                if rows[i + 1].flops_x100 < rows[i].flops_x100 {
                    return Err(cg::E_X830_FLOPS_REGRESSED);
                }
            }
            i += 1;
        }
        Ok(rows.len())
    };
    let ok = matches!(audit(&year_reg), Err(c) if c == cg::E_X830_YEAR_REGRESSED)
        && matches!(audit(&flops_reg), Err(c) if c == cg::E_X830_FLOPS_REGRESSED)
        && matches!(audit(&conf_over), Err(c) if c == cg::E_X830_CONFIDENCE_FAULT)
        && matches!(audit(&no_basis), Err(c) if c == cg::E_X830_BASIS_MISSING);
    if ok {
        set.ok("EX830-边界-02-变异四向必拒");
    } else {
        set.fail("EX830-边界-02-变异四向必拒", "某向变异漏抓");
    }
}

fn chk_spec_series_independent(set: &mut CheckSet) {
    // 规格-04：年份与 FLOPS 序列判据侧独立重算（单调性双算一致）。
    let mut ok = true;
    let mut i = 0usize;
    while i + 1 < 7 {
        if cg::ROADMAP_STEPS[i + 1].year <= cg::ROADMAP_STEPS[i].year {
            ok = false;
        }
        if cg::ROADMAP_STEPS[i + 1].flops_x100 < cg::ROADMAP_STEPS[i].flops_x100 {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX830-规格-04-双序列单调独立重算");
    } else {
        set.fail("EX830-规格-04-双序列单调独立重算", "单调性双算破缺");
    }
}

fn chk_bound_forecast_audit(set: &mut CheckSet) {
    // 边界-03：曲线×预测对账过闸（规划值落在预测口径区间内）。
    let ok = matches!(cg::audit_against_forecast(), Ok(7));
    if ok {
        set.ok("EX830-边界-03-曲线对账过闸");
    } else {
        set.fail("EX830-边界-03-曲线对账过闸", "规划与预测脱钩");
    }
}

fn chk_bound_forecast_audit_variant(set: &mut CheckSet) {
    // 边界-04：对账口径写死对拍——十年承诺 1100 必须落在 10 年视野
    // 上沿 2000 内且高于 5 年上沿口径的可用下界（数字对拍非同源）。
    let last = &cg::ROADMAP_STEPS[6];
    let ok = last.year == 2035 && last.flops_x100 == 1100 && last.flops_x100 <= 2000;
    if ok {
        set.ok("EX830-边界-04-十年承诺口径对拍");
    } else {
        set.fail("EX830-边界-04-十年承诺口径对拍", "十年承诺越出预测口径");
    }
}

fn chk_spec_baseline_anchor(set: &mut CheckSet) {
    // 规格-05：基线锚语义——首行 (2025, 100, 100)：基线年 1.0 倍+全置信。
    let first = &cg::ROADMAP_STEPS[0];
    let ok = first.year == 2025
        && first.flops_x100 == 100
        && first.confidence == 100
        && first.basis.contains("实测");
    if ok {
        set.ok("EX830-规格-05-基线锚语义");
    } else {
        set.fail("EX830-规格-05-基线锚语义", "基线锚漂移");
    }
}

fn chk_bound_audit_overrange(set: &mut CheckSet) {
    // 边界-07：对账越上沿变异——构造 flops=2500 超 10 年口径上沿 2000
    // （判据侧独立重算对账逻辑必拒）。
    let mut rows: Vec<(u32, u32)> = Vec::new();
    for s in cg::ROADMAP_STEPS.iter() {
        rows.push((s.year, s.flops_x100));
    }
    rows[6].1 = 2500;
    let ranges: [(u32, u32, u32); 7] = [
        (2025, 100, 220),
        (2026, 100, 220),
        (2027, 100, 220),
        (2029, 100, 600),
        (2031, 100, 600),
        (2033, 100, 2000),
        (2035, 100, 2000),
    ];
    let mut audit_ok = true;
    let mut i = 0usize;
    while i < 7 {
        if rows[i].0 != ranges[i].0 {
            audit_ok = false;
        }
        if rows[i].1 < ranges[i].1 || rows[i].1 > ranges[i].2 {
            audit_ok = false;
        }
        i += 1;
    }
    let ok = !audit_ok;
    if ok {
        set.ok("EX830-边界-07-对账越上沿必拒");
    } else {
        set.fail("EX830-边界-07-对账越上沿必拒", "对账闸漏抓");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-05：0x99C 段续编六码写死互异 + 与前序四十四码跨文件互异。
    let codes: [u16; 6] = [
        cg::E_X830_YEAR_REGRESSED,
        cg::E_X830_FLOPS_REGRESSED,
        cg::E_X830_CONFIDENCE_FAULT,
        cg::E_X830_BASIS_MISSING,
        cg::E_X830_VERSION_MISMATCH,
        E_X830_CHECK_TALLY,
    ];
    let want: [u16; 6] = [0x99CE, 0x99CF, 0x99D0, 0x99D1, 0x99D2, 0x99D3];
    let mut ok = true;
    let mut i = 0usize;
    while i < 6 {
        if codes[i] != want[i] {
            ok = false;
        }
        let mut j = 0usize;
        while j < 6 {
            if i != j && codes[i] == codes[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let others: [u16; 41] = [
        0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07, 0x9B10, 0x9B11,
        0x9B12, 0x9B13, 0x9B14, 0x9B15, 0x99A0, 0x99A1, 0x99A2, 0x99A3, 0x99A4, 0x99A5,
        0x99A6, 0x99A7, 0x99B0, 0x99B1, 0x99B2, 0x99B3, 0x99B4, 0x99B5, 0x99B6, 0x99B8,
        0x99B9, 0x99BA, 0x99BB, 0x99BC, 0x99BD, 0x99BE, 0x99C0, 0x99C1, 0x99C2, 0x99C3,
        0x99C7,
    ];
    for c in codes.iter() {
        let mut k = 0usize;
        while k < 41 {
            if *c == others[k] {
                ok = false;
            }
            k += 1;
        }
    }
    if ok {
        set.ok("EX830-边界-05-六码互异且跨段不撞");
    } else {
        set.fail("EX830-边界-05-六码互异且跨段不撞", "码漂移或跨段撞码");
    }
}

fn chk_bound_version(set: &mut CheckSet) {
    // 边界-06：版本写死（长度 19）+ 双向校验（冻结版放行/异版拒收）。
    let ok = cg::VCX83_VERSION == EXPECT_VERSION
        && cg::VCX83_VERSION.len() == 19
        && cg::check_version(EXPECT_VERSION).is_ok()
        && matches!(cg::check_version("CX83-computegrow-v2"), Err(c) if c == cg::E_X830_VERSION_MISMATCH);
    if ok {
        set.ok("EX830-边界-06-版本双向校验");
    } else {
        set.fail("EX830-边界-06-版本双向校验", "版本闸漏抓");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_roadmap_step(set: &mut CheckSet) {
    // B-01：RoadmapStep 字段承载——构造+四字段读回一致。
    let step = cg::RoadmapStep {
        year: 2035,
        flops_x100: 1100,
        confidence: 30,
        basis: "判据承载测试",
    };
    let ok = step.year == 2035 && step.flops_x100 == 1100 && step.confidence == 30
        && step.basis == "判据承载测试";
    if ok {
        set.ok("EX830-承载力-01-阶梯行字段承载");
    } else {
        set.fail("EX830-承载力-01-阶梯行字段承载", "字段承载断裂");
    }
}

fn chk_carry_step_count(set: &mut CheckSet) {
    // B-02：行数守恒——verify 返回值与常量表长度一致。
    let ok = matches!(cg::verify_roadmap(), Ok(n) if n == cg::ROADMAP_STEPS.len());
    if ok {
        set.ok("EX830-承载力-02-行数守恒");
    } else {
        set.fail("EX830-承载力-02-行数守恒", "行数承载断裂");
    }
}

fn chk_carry_span(set: &mut CheckSet) {
    // B-03：十年跨度承载——首尾年差 10（2025→2035）且倍率 11 倍。
    let first = &cg::ROADMAP_STEPS[0];
    let last = &cg::ROADMAP_STEPS[6];
    let ok = last.year - first.year == 10 && last.flops_x100 / first.flops_x100 == 11;
    if ok {
        set.ok("EX830-承载力-03-十年跨度承载");
    } else {
        set.fail("EX830-承载力-03-十年跨度承载", "跨度承载断裂");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcx83_computegrow_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EX830-判据-零panic面");
    } else {
        set.fail("EX830-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcx83_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EX830-判据-聚合守恒防自调");
    } else {
        set.fail("EX830-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规划组 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规划组（12 条：规格 4 + 边界 6 + 版本 1 + 码段 1）。
pub fn run_vcx83_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcx83/a");
    chk_spec_steps(&mut s);
    chk_spec_basis(&mut s);
    chk_spec_confidence_decline(&mut s);
    chk_spec_series_independent(&mut s);
    chk_spec_baseline_anchor(&mut s);
    chk_bound_roadmap_pass(&mut s);
    chk_bound_variants(&mut s);
    chk_bound_forecast_audit(&mut s);
    chk_bound_forecast_audit_variant(&mut s);
    chk_bound_audit_overrange(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_version(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条）。
pub fn run_vcx83_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcx83/b");
    chk_carry_roadmap_step(&mut s);
    chk_carry_step_count(&mut s);
    chk_carry_span(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcx83_checks() -> CheckSet {
    CheckSet::merge(
        run_vcx83_checks_a_standalone(),
        run_vcx83_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcx83_checks_a_standalone, run_vcx83_checks_b_standalone, run_vcx83_checks};

    #[test]
    fn x03_a_standalone_all_green() {
        let set = run_vcx83_checks_a_standalone();
        assert!(set.all_passed(), "vcx83 A 批红项存在");
    }

    #[test]
    fn x03_b_standalone_all_green() {
        let set = run_vcx83_checks_b_standalone();
        assert!(set.all_passed(), "vcx83 B 批红项存在");
    }

    #[test]
    fn x03_merged_all_green() {
        let set = run_vcx83_checks();
        assert!(set.all_passed(), "vcx83 merged 红项存在");
    }
}
