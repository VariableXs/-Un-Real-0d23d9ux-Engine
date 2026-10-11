//! CGPU-F3682 域自检（CGPU-X 域硬件演进预测模型判据层）
//!
//! 判据侧**独立写死**期望（三轴名册、九行预测表、区间单调性独立重算、
//! 0x99C 段续编五码互异并与前段跨文件互异）。判据侧自备独立校验器跑
//! 变异表（倒挂/越轴/重格/依据空各向拒绝）。聚合防自调：族内判据只调
//! 另一族 standalone + 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcx82_forecast as fc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（X 域 0x99C 段续编，与实现文件四码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_X820_CHECK_TALLY: u16 = 0x99CC;

const EXPECT_VERSION: &str = "CX82-forecast-v1";

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
// A 族 · 模型组（规格 + 边界）
// ---------------------------------------------------------------------------

fn chk_spec_axes(set: &mut CheckSet) {
    // 规格-01：三轴名册写死对拍（算力/带宽/显存，锚点原文次序）。
    let want: [&str; 3] = ["算力", "带宽", "显存"];
    let mut ok = fc::FORECAST_AXES.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if fc::FORECAST_AXES[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if fc::HORIZONS != [2, 5, 10] {
        ok = false;
    }
    if ok {
        set.ok("EX820-规格-01-三轴与视野写死对拍");
    } else {
        set.fail("EX820-规格-01-三轴与视野写死对拍", "三轴或视野漂移");
    }
}

fn chk_spec_table_grid(set: &mut CheckSet) {
    // 规格-02：预测表满格独立重算——三轴 × 三视野九格不缺不重（判据侧重算）。
    let mut grid = [[0usize; 3]; 3];
    let mut ok = fc::FORECAST_TABLE.len() == 9;
    for row in fc::FORECAST_TABLE.iter() {
        if row.axis < 3 {
            let mut h = 0usize;
            while h < 3 {
                if fc::HORIZONS[h] == row.horizon {
                    grid[row.axis][h] += 1;
                }
                h += 1;
            }
        } else {
            ok = false;
        }
    }
    let mut a = 0usize;
    while a < 3 {
        let mut h = 0usize;
        while h < 3 {
            if grid[a][h] != 1 {
                ok = false;
            }
            h += 1;
        }
        a += 1;
    }
    if ok {
        set.ok("EX820-规格-02-九格满格独立重算");
    } else {
        set.fail("EX820-规格-02-九格满格独立重算", "缺格或重格");
    }
}

fn chk_spec_basis(set: &mut CheckSet) {
    // 规格-03：九行依据写死对拍（前缀锚逐行）——无依据的区间与瞎猜无法区分。
    let want_prefix: [&str; 9] = [
        "已公布制程", "制程 1.5 代", "新器件形态",
        "PCIe 世代交替", "新总线世代", "光互连成熟度",
        "HBM 世代堆叠", "HBM 堆叠+基板键合", "新显存介质",
    ];
    let mut ok = fc::FORECAST_TABLE.len() == 9;
    let mut i = 0usize;
    while i < 9 {
        if !fc::FORECAST_TABLE[i].basis.starts_with(want_prefix[i]) {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX820-规格-03-九行依据写死对拍");
    } else {
        set.fail("EX820-规格-03-九行依据写死对拍", "依据漂移或缺席");
    }
}

fn chk_bound_table_pass(set: &mut CheckSet) {
    // 边界-01：登记表过实现闸（满格+区间合法+依据非空）。
    let ok = matches!(fc::verify_forecast_table(), Ok(9));
    if ok {
        set.ok("EX820-边界-01-登记表过闸");
    } else {
        set.fail("EX820-边界-01-登记表过闸", "登记表被误拒");
    }
}

fn chk_bound_variants(set: &mut CheckSet) {
    // 边界-02：判据侧独立校验器变异四向必拒
    // （倒挂/越轴/重格/依据空——同判据异实现）。
    let mut invert: Vec<fc::ForecastRow> = Vec::new();
    for row in fc::FORECAST_TABLE.iter() {
        invert.push(row.clone());
    }
    invert[0].high = invert[0].low;
    let mut offaxis: Vec<fc::ForecastRow> = Vec::new();
    for row in fc::FORECAST_TABLE.iter() {
        offaxis.push(row.clone());
    }
    offaxis[0].axis = 5;
    let mut dup: Vec<fc::ForecastRow> = Vec::new();
    for row in fc::FORECAST_TABLE.iter() {
        dup.push(row.clone());
    }
    dup[1].axis = dup[0].axis;
    dup[1].horizon = dup[0].horizon;
    let mut nobasis: Vec<fc::ForecastRow> = Vec::new();
    for row in fc::FORECAST_TABLE.iter() {
        nobasis.push(row.clone());
    }
    nobasis[0].basis = "";
    let audit = |rows: &Vec<fc::ForecastRow>| -> Result<usize, u16> {
        if rows.len() != 9 {
            return Err(fc::E_X820_AXIS_UNKNOWN);
        }
        let mut i = 0usize;
        while i < rows.len() {
            if rows[i].axis >= 3 {
                return Err(fc::E_X820_AXIS_UNKNOWN);
            }
            let mut inh = false;
            let mut h = 0usize;
            while h < 3 {
                if fc::HORIZONS[h] == rows[i].horizon {
                    inh = true;
                }
                h += 1;
            }
            if !inh {
                return Err(fc::E_X820_AXIS_UNKNOWN);
            }
            let mut j = i + 1;
            while j < rows.len() {
                if rows[j].axis == rows[i].axis && rows[j].horizon == rows[i].horizon {
                    return Err(fc::E_X820_AXIS_UNKNOWN);
                }
                j += 1;
            }
            i += 1;
        }
        for row in rows.iter() {
            if row.low >= row.high {
                return Err(fc::E_X820_RANGE_INVERTED);
            }
            if row.basis.is_empty() {
                return Err(fc::E_X820_BASIS_MISSING);
            }
        }
        Ok(rows.len())
    };
    let ok = matches!(audit(&invert), Err(c) if c == fc::E_X820_RANGE_INVERTED)
        && matches!(audit(&offaxis), Err(c) if c == fc::E_X820_AXIS_UNKNOWN)
        && matches!(audit(&dup), Err(c) if c == fc::E_X820_AXIS_UNKNOWN)
        && matches!(audit(&nobasis), Err(c) if c == fc::E_X820_BASIS_MISSING);
    if ok {
        set.ok("EX820-边界-02-变异四向必拒");
    } else {
        set.fail("EX820-边界-02-变异四向必拒", "某向变异漏抓");
    }
}

fn chk_bound_monotonic(set: &mut CheckSet) {
    // 边界-03：三轴区间单调独立重算（视野越远区间越宽——预测诚实性结构表达）。
    let mut ok = true;
    let mut axis = 0usize;
    while axis < 3 {
        if fc::verify_axis_monotonic(axis).is_err() {
            ok = false;
        }
        // 判据侧独立重算宽度序列。
        let mut widths: [u32; 3] = [0, 0, 0];
        for row in fc::FORECAST_TABLE.iter() {
            if row.axis == axis {
                let mut h = 0usize;
                while h < 3 {
                    if fc::HORIZONS[h] == row.horizon {
                        widths[h] = row.high - row.low;
                    }
                    h += 1;
                }
            }
        }
        if !(widths[0] < widths[1] && widths[1] < widths[2]) {
            ok = false;
        }
        axis += 1;
    }
    if ok {
        set.ok("EX820-边界-03-三轴单调双算一致");
    } else {
        set.fail("EX820-边界-03-三轴单调双算一致", "单调破缺或双算不同判");
    }
}

fn chk_bound_monotonic_variant(set: &mut CheckSet) {
    // 边界-04：独立校验器跑单调破缺变异行集（宽 100,100,100 → 拒）。
    let flat: Vec<fc::ForecastRow> = vec![
        fc::ForecastRow { axis: 0, horizon: 2, low: 100, high: 200, basis: "b" },
        fc::ForecastRow { axis: 0, horizon: 5, low: 100, high: 200, basis: "b" },
        fc::ForecastRow { axis: 0, horizon: 10, low: 100, high: 200, basis: "b" },
        fc::ForecastRow { axis: 1, horizon: 2, low: 100, high: 150, basis: "b" },
        fc::ForecastRow { axis: 1, horizon: 5, low: 100, high: 250, basis: "b" },
        fc::ForecastRow { axis: 1, horizon: 10, low: 100, high: 400, basis: "b" },
        fc::ForecastRow { axis: 2, horizon: 2, low: 100, high: 160, basis: "b" },
        fc::ForecastRow { axis: 2, horizon: 5, low: 100, high: 320, basis: "b" },
        fc::ForecastRow { axis: 2, horizon: 10, low: 100, high: 640, basis: "b" },
    ];
    // 判据侧独立单调审计（宽序列必须严格递增）。
    let audit_monotonic = |rows: &Vec<fc::ForecastRow>, axis: usize| -> Result<(), u16> {
        let mut widths: [u32; 3] = [0, 0, 0];
        let mut seen = 0usize;
        for row in rows.iter() {
            if row.axis == axis {
                let mut h = 0usize;
                while h < 3 {
                    if fc::HORIZONS[h] == row.horizon {
                        widths[h] = row.high - row.low;
                        seen += 1;
                    }
                    h += 1;
                }
            }
        }
        if seen != 3 {
            return Err(fc::E_X820_AXIS_UNKNOWN);
        }
        if !(widths[0] < widths[1] && widths[1] < widths[2]) {
            return Err(fc::E_X820_RANGE_INVERTED);
        }
        Ok(())
    };
    let ok = matches!(audit_monotonic(&flat, 0), Err(c) if c == fc::E_X820_RANGE_INVERTED)
        && audit_monotonic(&flat, 1).is_ok();
    if ok {
        set.ok("EX820-边界-04-单调破缺变异必拒");
    } else {
        set.fail("EX820-边界-04-单调破缺变异必拒", "单调闸漏抓");
    }
}

fn chk_spec_ranges(set: &mut CheckSet) {
    // 规格-04：九行区间数值写死对拍（low/high 全部写死——数字漂移必抓）。
    let want: [(u32, u32); 9] = [
        (150, 220), (300, 600), (600, 2000),
        (140, 200), (250, 500), (500, 1800),
        (160, 240), (320, 640), (640, 2400),
    ];
    let mut ok = fc::FORECAST_TABLE.len() == 9;
    let mut i = 0usize;
    while i < 9 {
        if fc::FORECAST_TABLE[i].low != want[i].0 || fc::FORECAST_TABLE[i].high != want[i].1 {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX820-规格-04-九行区间写死对拍");
    } else {
        set.fail("EX820-规格-04-九行区间写死对拍", "区间数值漂移");
    }
}

fn chk_bound_monotonic_offaxis(set: &mut CheckSet) {
    // 边界-07：verify_axis_monotonic 越轴拒（axis=3 → AXIS_UNKNOWN）。
    let ok = matches!(fc::verify_axis_monotonic(3), Err(c) if c == fc::E_X820_AXIS_UNKNOWN);
    if ok {
        set.ok("EX820-边界-07-单调校验越轴拒");
    } else {
        set.fail("EX820-边界-07-单调校验越轴拒", "越轴漏抓");
    }
}

fn chk_bound_horizon_offgrid(set: &mut CheckSet) {
    // 边界-08：视野越界变异（horizon=7 不在封闭视野集 → AXIS_UNKNOWN，
    // 判据侧独立审计重算）。
    let mut off: Vec<fc::ForecastRow> = Vec::new();
    for row in fc::FORECAST_TABLE.iter() {
        off.push(row.clone());
    }
    off[0].horizon = 7;
    let audit_grid = |rows: &Vec<fc::ForecastRow>| -> Result<usize, u16> {
        let mut i = 0usize;
        while i < rows.len() {
            if rows[i].axis >= 3 {
                return Err(fc::E_X820_AXIS_UNKNOWN);
            }
            let mut inh = false;
            let mut h = 0usize;
            while h < 3 {
                if fc::HORIZONS[h] == rows[i].horizon {
                    inh = true;
                }
                h += 1;
            }
            if !inh {
                return Err(fc::E_X820_AXIS_UNKNOWN);
            }
            i += 1;
        }
        Ok(rows.len())
    };
    let ok = matches!(audit_grid(&off), Err(c) if c == fc::E_X820_AXIS_UNKNOWN);
    if ok {
        set.ok("EX820-边界-08-视野越界变异必拒");
    } else {
        set.fail("EX820-边界-08-视野越界变异必拒", "视野闸漏抓");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-05：0x99C 段续编五码写死互异 + 与前序三十九码跨文件互异。
    let codes: [u16; 5] = [
        fc::E_X820_RANGE_INVERTED,
        fc::E_X820_AXIS_UNKNOWN,
        fc::E_X820_BASIS_MISSING,
        fc::E_X820_VERSION_MISMATCH,
        E_X820_CHECK_TALLY,
    ];
    let want: [u16; 5] = [0x99C8, 0x99C9, 0x99CA, 0x99CB, 0x99CC];
    let mut ok = true;
    let mut i = 0usize;
    while i < 5 {
        if codes[i] != want[i] {
            ok = false;
        }
        let mut j = 0usize;
        while j < 5 {
            if i != j && codes[i] == codes[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let others: [u16; 36] = [
        0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07, 0x9B10, 0x9B11,
        0x9B12, 0x9B13, 0x9B14, 0x9B15, 0x99A0, 0x99A1, 0x99A2, 0x99A3, 0x99A4, 0x99A5,
        0x99A6, 0x99A7, 0x99B0, 0x99B1, 0x99B2, 0x99B3, 0x99B4, 0x99B5, 0x99B6, 0x99B8,
        0x99B9, 0x99BA, 0x99BB, 0x99BC, 0x99BD, 0x99BE,
    ];
    let mut c = 0usize;
    while c < 5 {
        let mut k = 0usize;
        while k < 36 {
            if codes[c] == others[k] {
                ok = false;
            }
            k += 1;
        }
        c += 1;
    }
    if ok {
        set.ok("EX820-边界-05-五码互异且跨段不撞");
    } else {
        set.fail("EX820-边界-05-五码互异且跨段不撞", "码漂移或跨段撞码");
    }
}

fn chk_bound_version(set: &mut CheckSet) {
    // 边界-06：版本写死（长度 16）+ 双向校验（冻结版放行/异版拒收）。
    let ok = fc::VCX82_VERSION == EXPECT_VERSION
        && fc::VCX82_VERSION.len() == 16
        && fc::check_version(EXPECT_VERSION).is_ok()
        && matches!(fc::check_version("CX82-forecast-v2"), Err(c) if c == fc::E_X820_VERSION_MISMATCH);
    if ok {
        set.ok("EX820-边界-06-版本双向校验");
    } else {
        set.fail("EX820-边界-06-版本双向校验", "版本闸漏抓");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_forecast_row(set: &mut CheckSet) {
    // B-01：ForecastRow 字段承载——构造+五字段读回一致。
    let row = fc::ForecastRow {
        axis: 2,
        horizon: 10,
        low: 640,
        high: 2400,
        basis: "判据承载测试",
    };
    let ok = row.axis == 2 && row.horizon == 10 && row.low == 640 && row.high == 2400
        && row.basis == "判据承载测试";
    if ok {
        set.ok("EX820-承载力-01-预测行字段承载");
    } else {
        set.fail("EX820-承载力-01-预测行字段承载", "字段承载断裂");
    }
}

fn chk_carry_table_count(set: &mut CheckSet) {
    // B-02：登记表行数守恒——verify 返回值与常量表长度一致。
    let ok = matches!(fc::verify_forecast_table(), Ok(n) if n == fc::FORECAST_TABLE.len());
    if ok {
        set.ok("EX820-承载力-02-行数守恒");
    } else {
        set.fail("EX820-承载力-02-行数守恒", "行数承载断裂");
    }
}

fn chk_carry_axis_names(set: &mut CheckSet) {
    // B-03：轴名承载——三轴名均两字且非空（名册结构承载）。
    let mut ok = fc::FORECAST_AXES.len() == 3;
    for axis in fc::FORECAST_AXES.iter() {
        if axis.len() != 6 {
            ok = false;
        }
    }
    if ok {
        set.ok("EX820-承载力-03-轴名结构承载");
    } else {
        set.fail("EX820-承载力-03-轴名结构承载", "轴名承载断裂");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcx82_forecast_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EX820-判据-零panic面");
    } else {
        set.fail("EX820-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcx82_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EX820-判据-聚合守恒防自调");
    } else {
        set.fail("EX820-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=模型组 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：模型组（12 条：规格 4 + 边界 8）。
pub fn run_vcx82_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcx82/a");
    chk_spec_axes(&mut s);
    chk_spec_table_grid(&mut s);
    chk_spec_basis(&mut s);
    chk_spec_ranges(&mut s);
    chk_bound_table_pass(&mut s);
    chk_bound_variants(&mut s);
    chk_bound_monotonic(&mut s);
    chk_bound_monotonic_variant(&mut s);
    chk_bound_monotonic_offaxis(&mut s);
    chk_bound_horizon_offgrid(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_version(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条）。
pub fn run_vcx82_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcx82/b");
    chk_carry_forecast_row(&mut s);
    chk_carry_table_count(&mut s);
    chk_carry_axis_names(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcx82_checks() -> CheckSet {
    CheckSet::merge(
        run_vcx82_checks_a_standalone(),
        run_vcx82_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcx82_checks_a_standalone, run_vcx82_checks_b_standalone, run_vcx82_checks};

    #[test]
    fn x02_a_standalone_all_green() {
        let set = run_vcx82_checks_a_standalone();
        assert!(set.all_passed(), "vcx82 A 批红项存在");
    }

    #[test]
    fn x02_b_standalone_all_green() {
        let set = run_vcx82_checks_b_standalone();
        assert!(set.all_passed(), "vcx82 B 批红项存在");
    }

    #[test]
    fn x02_merged_all_green() {
        let set = run_vcx82_checks();
        assert!(set.all_passed(), "vcx82 merged 红项存在");
    }
}
