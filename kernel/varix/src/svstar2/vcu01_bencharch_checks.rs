//! CGPU-F3201 域自检（CGPU-U 域开工与 Bench 总架构判据层）
//!
//! 判据侧**独立写死**期望（移交包七件、衔接包两件、定位声明、理念呼应、
//! 四段封闭表与章程、T10 兑现三条、0x99A 段七码写死互异并与 0x9B 段
//! 十四码跨文件互异复核）。判据侧自备独立校验器跑变异章程
//! （铁律洗除/空章程各向拒绝）。聚合防自调：族内判据只调另一族
//! standalone + 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcu01_bencharch as ba;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（U 域 0x99A 段续编，与实现文件七码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_U010_CHECK_TALLY: u16 = 0x99A7;

const EXPECT_VERSION: &str = "CU01-bencharch-v1";

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
// A 族 · 签收组 + 定位组 + 四段组 + 兑现组（规格 + 边界）
// ---------------------------------------------------------------------------

fn chk_spec_handover_seven(set: &mut CheckSet) {
    // 规格-01：移交包七件与写死名单逐项相等（封闭清单对拍）。
    let want: [&str; 7] = [
        "接口冻结", "契约", "资产", "基线", "遗留", "Bench 衔接包", "教训十条",
    ];
    let mut ok = ba::HANDOVER_SEVEN.len() == 7;
    let mut i = 0usize;
    while i < 7 {
        if ba::HANDOVER_SEVEN[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EU010-规格-01-移交包七件写死对拍");
    } else {
        set.fail("EU010-规格-01-移交包七件写死对拍", "七件名册漂移或缺件");
    }
}

fn chk_spec_link_package(set: &mut CheckSet) {
    // 规格-02：衔接包两件名目写死对拍（兑现确认的对照基准）。
    let ok = ba::LINK_PACKAGE.len() == 2
        && ba::LINK_PACKAGE[0] == "对接协议基准项"
        && ba::LINK_PACKAGE[1] == "性能基准口径";
    if ok {
        set.ok("EU010-规格-02-衔接包两件写死对拍");
    } else {
        set.fail("EU010-规格-02-衔接包两件写死对拍", "衔接包名目漂移");
    }
}

fn chk_bound_missing_item(set: &mut CheckSet) {
    // 边界-01：缺一件（位 3 基线 / 位 5 衔接包各自缺席）→ 拒收 0x99A0。
    let hole_a = [true, true, true, false, true, true, true];
    let hole_b = [true, true, true, true, true, false, true];
    let ok = matches!(ba::sign_off(hole_a, true), Err(c) if c == 0x99A0)
        && matches!(ba::sign_off(hole_b, true), Err(c) if c == 0x99A0);
    if ok {
        set.ok("EU010-边界-01-缺件拒收两侧钉死");
    } else {
        set.fail("EU010-边界-01-缺件拒收两侧钉死", "缺件闸漏抓");
    }
}

fn chk_bound_link_unconfirmed(set: &mut CheckSet) {
    // 边界-02：七件齐但衔接确认缺席 → 0x99A1。
    let all = [true, true, true, true, true, true, true];
    let ok = matches!(ba::sign_off(all, false), Err(c) if c == 0x99A1);
    if ok {
        set.ok("EU010-边界-02-衔接未确认拒收");
    } else {
        set.fail("EU010-边界-02-衔接未确认拒收", "衔接确认闸漏抓");
    }
}

fn chk_spec_signoff_receipt(set: &mut CheckSet) {
    // 规格-03：全齐+衔接确认 → 收讫，回执三字段齐备（收 7/确认/版本）。
    let all = [true, true, true, true, true, true, true];
    let ok = match ba::sign_off(all, true) {
        Ok(r) => r.items_signed == 7 && r.link_confirmed && r.version == EXPECT_VERSION,
        Err(_) => false,
    };
    if ok {
        set.ok("EU010-规格-03-全齐收讫回执齐备");
    } else {
        set.fail("EU010-规格-03-全齐收讫回执齐备", "回执字段漂移");
    }
}

fn chk_spec_positioning(set: &mut CheckSet) {
    // 规格-04：定位声明与理念呼应写死对拍（「实测」为不可删字）。
    let ok = ba::POSITIONING == "实测认证不凭文档声明"
        && ba::CGPU_ECHO == "与等价优先同源：基准数字是唯一凭证";
    if ok {
        set.ok("EU010-规格-04-定位与呼应写死对拍");
    } else {
        set.fail("EU010-规格-04-定位与呼应写死对拍", "定位声明漂移或丢实测");
    }
}

fn chk_bound_positioning_gate(set: &mut CheckSet) {
    // 边界-03：实测声明放行 / 纯文档声明失守 0x99A2（两侧钉死；反例不含「实测」）。
    let ok = ba::verify_positioning("实测认证不凭文档声明").is_ok()
        && matches!(ba::verify_positioning("文档背书不凭基准声明"), Err(c) if c == 0x99A2);
    if ok {
        set.ok("EU010-边界-03-定位校验两侧钉死");
    } else {
        set.fail("EU010-边界-03-定位校验两侧钉死", "纯文档声明漏抓");
    }
}

fn chk_spec_segments(set: &mut CheckSet) {
    // 规格-05：四段封闭表逐值对拍（场景/运行/计分/发布，次序固定）。
    let want = [
        ba::BenchSegment::Scene,
        ba::BenchSegment::Run,
        ba::BenchSegment::Score,
        ba::BenchSegment::Publish,
    ];
    let mut ok = ba::BENCH_SEGMENTS.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        if ba::BENCH_SEGMENTS[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EU010-规格-05-四段封闭表对拍");
    } else {
        set.fail("EU010-规格-05-四段封闭表对拍", "四段漂移或次序错");
    }
}

fn chk_bound_charters_both_sides(set: &mut CheckSet) {
    // 边界-04：登记章程过闸 / 铁律洗除与空章程双向必拒（0x99A4/0x99A3）。
    let stripped: [&str; 4] = [
        "场景：基准场景库——负载的固化形态",
        "运行：运行器——确定性执行与采样复现",
        "计分：三分计分体系——口径先行仲裁在后",
        "发布：基准发布与回归——数字出证且看守",
    ];
    let blank: [&str; 4] = [
        "场景：基准场景库——真实负载的固化形态",
        "运行：确定性执行",
        "",
        "发布：数字出证",
    ];
    let ok = ba::verify_bench_charters(ba::BENCH_CHARTER).is_ok()
        && matches!(ba::verify_bench_charters(stripped), Err(c) if c == 0x99A4)
        && matches!(ba::verify_bench_charters(blank), Err(c) if c == 0x99A3);
    if ok {
        set.ok("EU010-边界-04-章程闸两侧钉死");
    } else {
        set.fail("EU010-边界-04-章程闸两侧钉死", "铁律洗除或空章程漏抓");
    }
}

fn chk_spec_t10_fulfillment(set: &mut CheckSet) {
    // 规格-06：T10 兑现三条写死对拍（一致性/吞吐/延迟）。
    let want: [&str; 3] = [
        "协议一致性基准：T 域对接协议的互操作一致性计分口径",
        "吞吐基准：单位时间渲染作业完成量的实测口径",
        "延迟基准：端到端作业延迟的实测口径与采样规则",
    ];
    let mut ok = ba::T10_FULFILLMENT.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if ba::T10_FULFILLMENT[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if !matches!(ba::verify_t10_fulfilled(), Ok(r) if r.items == 3 && r.confirmed) {
        ok = false;
    }
    if ok {
        set.ok("EU010-规格-06-兑现三条写死对拍");
    } else {
        set.fail("EU010-规格-06-兑现三条写死对拍", "T10 兑现漂移或空欠");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-05：0x99A 段八码（本单）写死互异 + 与 0x9B 段十四码跨文件互异。
    let codes: [u16; 8] = [
        ba::E_U010_MISSING_ITEM,
        ba::E_U010_LINK_UNCONFIRMED,
        ba::E_U010_POSITIONING_FAULT,
        ba::E_U010_SEGMENT_INCOMPLETE,
        ba::E_U010_SCENE_LOAD_MISSING,
        ba::E_U010_T10_UNFULFILLED,
        ba::E_U010_VERSION_MISMATCH,
        E_U010_CHECK_TALLY,
    ];
    let want: [u16; 8] = [0x99A0, 0x99A1, 0x99A2, 0x99A3, 0x99A4, 0x99A5, 0x99A6, 0x99A7];
    let mut ok = true;
    let mut i = 0usize;
    while i < 8 {
        if codes[i] != want[i] {
            ok = false;
        }
        let mut j = 0usize;
        while j < 8 {
            if i != j && codes[i] == codes[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    // 跨文件：与 S 域 0x9B 段十四码互异（两段不得撞码）。
    let s_codes: [u16; 14] = [
        0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07, 0x9B10, 0x9B11,
        0x9B12, 0x9B13, 0x9B14, 0x9B15,
    ];
    for c in codes.iter() {
        let mut k = 0usize;
        while k < 14 {
            if *c == s_codes[k] {
                ok = false;
            }
            k += 1;
        }
    }
    if ok {
        set.ok("EU010-边界-05-八码互异且与S域不撞");
    } else {
        set.fail("EU010-边界-05-八码互异且与S域不撞", "码漂移或跨段撞码");
    }
}

fn chk_bound_domain_ledger(set: &mut CheckSet) {
    // 边界-06：U 域开工总账——号段 (3201,3360)、总项 160（独立重算）。
    let recomputed = 3360 - 3201 + 1;
    let ok = ba::U_DOMAIN_RANGE == (3201, 3360)
        && ba::u_domain_total() == 160
        && recomputed == 160;
    if ok {
        set.ok("EU010-边界-06-开工总账160");
    } else {
        set.fail("EU010-边界-06-开工总账160", "号段或总账漂移");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_receipt_version(set: &mut CheckSet) {
    // B-01：签收回执版本承载——回执版本与冻结版同源且长度 17。
    let all = [true, true, true, true, true, true, true];
    let ok = match ba::sign_off(all, true) {
        Ok(r) => r.version.len() == 17 && r.version == ba::VCU01_VERSION,
        Err(_) => false,
    };
    if ok {
        set.ok("EU010-承载力-01-回执版本同源");
    } else {
        set.fail("EU010-承载力-01-回执版本同源", "版本承载断裂");
    }
}

fn chk_carry_t10_total(set: &mut CheckSet) {
    // B-02：兑现回执承载力——条目 3 且确认在场。
    let ok = matches!(ba::verify_t10_fulfilled(), Ok(r) if r.items == 3 && r.confirmed);
    if ok {
        set.ok("EU010-承载力-02-兑现条目承载");
    } else {
        set.fail("EU010-承载力-02-兑现条目承载", "兑现承载断裂");
    }
}

fn chk_carry_version_both_ways(set: &mut CheckSet) {
    // B-03：版本校验双向——冻结版放行，异版拒收 0x99A6。
    let ok = ba::check_version(EXPECT_VERSION).is_ok()
        && matches!(ba::check_version("CU01-bencharch-v2"), Err(c) if c == 0x99A6);
    if ok {
        set.ok("EU010-承载力-03-版本双向校验");
    } else {
        set.fail("EU010-承载力-03-版本双向校验", "版本闸漏抓");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcu01_bencharch_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EU010-判据-零panic面");
    } else {
        set.fail("EU010-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcu01_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EU010-判据-聚合守恒防自调");
    } else {
        set.fail("EU010-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=四组规格+边界 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：四组规格 + 边界（12 条）。
pub fn run_vcu01_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcu01/a");
    chk_spec_handover_seven(&mut s);
    chk_spec_link_package(&mut s);
    chk_bound_missing_item(&mut s);
    chk_bound_link_unconfirmed(&mut s);
    chk_spec_signoff_receipt(&mut s);
    chk_spec_positioning(&mut s);
    chk_bound_positioning_gate(&mut s);
    chk_spec_segments(&mut s);
    chk_bound_charters_both_sides(&mut s);
    chk_spec_t10_fulfillment(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_domain_ledger(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条）。
pub fn run_vcu01_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcu01/b");
    chk_carry_receipt_version(&mut s);
    chk_carry_t10_total(&mut s);
    chk_carry_version_both_ways(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcu01_checks() -> CheckSet {
    CheckSet::merge(
        run_vcu01_checks_a_standalone(),
        run_vcu01_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcu01_checks_a_standalone, run_vcu01_checks_b_standalone, run_vcu01_checks};

    #[test]
    fn u01_a_standalone_all_green() {
        let set = run_vcu01_checks_a_standalone();
        assert!(set.all_passed(), "vcu01 A 批红项存在");
    }

    #[test]
    fn u01_b_standalone_all_green() {
        let set = run_vcu01_checks_b_standalone();
        assert!(set.all_passed(), "vcu01 B 批红项存在");
    }

    #[test]
    fn u01_merged_all_green() {
        let set = run_vcu01_checks();
        assert!(set.all_passed(), "vcu01 merged 红项存在");
    }
}
