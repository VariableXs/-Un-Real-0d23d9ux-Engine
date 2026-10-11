//! CGPU-F3681 域自检（CGPU-X 域开工与十年硬件演进总架构判据层）
//!
//! 判据侧**独立写死**期望（移交包七件、衔接包两件、定位声明、演进纪律、
//! 四段封闭表与章程、W10 兑现三条、0x99C 段八码写死互异并与前三段
//! 跨文件互异复核）。聚合防自调：族内判据只调另一族 standalone +
//! 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcx81_hwevolve as he;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（X 域 0x99C 段续编，与实现文件七码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_X810_CHECK_TALLY: u16 = 0x99C7;

const EXPECT_VERSION: &str = "CX81-hwevolve-v1";

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
        "接口冻结", "契约", "资产", "基线", "遗留", "演进衔接包", "教训十条",
    ];
    let mut ok = he::HANDOVER_SEVEN.len() == 7;
    let mut i = 0usize;
    while i < 7 {
        if he::HANDOVER_SEVEN[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX810-规格-01-移交包七件写死对拍");
    } else {
        set.fail("EX810-规格-01-移交包七件写死对拍", "七件名册漂移或缺件");
    }
}

fn chk_spec_link_package(set: &mut CheckSet) {
    // 规格-02：衔接包两件名目写死对拍（兑现确认的对照基准）。
    let ok = he::LINK_PACKAGE.len() == 2
        && he::LINK_PACKAGE[0] == "文档章节演进预留"
        && he::LINK_PACKAGE[1] == "硬件演进观察口径";
    if ok {
        set.ok("EX810-规格-02-衔接包两件写死对拍");
    } else {
        set.fail("EX810-规格-02-衔接包两件写死对拍", "衔接包名目漂移");
    }
}

fn chk_bound_missing_item(set: &mut CheckSet) {
    // 边界-01：缺一件（位 3 基线 / 位 5 衔接包各自缺席）→ 拒收 0x99C0。
    let hole_a = [true, true, true, false, true, true, true];
    let hole_b = [true, true, true, true, true, false, true];
    let ok = matches!(he::sign_off(hole_a, true), Err(c) if c == 0x99C0)
        && matches!(he::sign_off(hole_b, true), Err(c) if c == 0x99C0);
    if ok {
        set.ok("EX810-边界-01-缺件拒收两侧钉死");
    } else {
        set.fail("EX810-边界-01-缺件拒收两侧钉死", "缺件闸漏抓");
    }
}

fn chk_bound_link_unconfirmed(set: &mut CheckSet) {
    // 边界-02：七件齐但衔接确认缺席 → 0x99C1。
    let all = [true, true, true, true, true, true, true];
    let ok = matches!(he::sign_off(all, false), Err(c) if c == 0x99C1);
    if ok {
        set.ok("EX810-边界-02-衔接未确认拒收");
    } else {
        set.fail("EX810-边界-02-衔接未确认拒收", "衔接确认闸漏抓");
    }
}

fn chk_spec_signoff_receipt(set: &mut CheckSet) {
    // 规格-03：全齐+衔接确认 → 收讫，回执三字段齐备（收 7/确认/版本）。
    let all = [true, true, true, true, true, true, true];
    let ok = match he::sign_off(all, true) {
        Ok(r) => r.items_signed == 7 && r.link_confirmed && r.version == EXPECT_VERSION,
        Err(_) => false,
    };
    if ok {
        set.ok("EX810-规格-03-全齐收讫回执齐备");
    } else {
        set.fail("EX810-规格-03-全齐收讫回执齐备", "回执字段漂移");
    }
}

fn chk_spec_positioning(set: &mut CheckSet) {
    // 规格-04：定位声明与演进纪律写死对拍（「前瞻」为不可删字）。
    let ok = he::POSITIONING == "十年硬件浪潮下的前瞻设计"
        && he::EVOLVE_DISCIPLINE == "预测给区间不押单点，预留给接口不给实现";
    if ok {
        set.ok("EX810-规格-04-定位与纪律写死对拍");
    } else {
        set.fail("EX810-规格-04-定位与纪律写死对拍", "定位声明漂移或丢前瞻");
    }
}

fn chk_bound_positioning_gate(set: &mut CheckSet) {
    // 边界-03：前瞻声明放行 / 追赶式声明失守 0x99C2（反例不含「前瞻」）。
    let ok = he::verify_positioning("十年硬件浪潮下的前瞻设计").is_ok()
        && matches!(he::verify_positioning("跟随当前硬件的设计"), Err(c) if c == 0x99C2);
    if ok {
        set.ok("EX810-边界-03-定位校验两侧钉死");
    } else {
        set.fail("EX810-边界-03-定位校验两侧钉死", "追赶式声明漏抓");
    }
}

fn chk_spec_segments(set: &mut CheckSet) {
    // 规格-05：四段封闭表逐值对拍（预测/规划/预留/风险，次序固定）。
    let want = [
        he::EvolveSegment::Forecast,
        he::EvolveSegment::Roadmap,
        he::EvolveSegment::Reserve,
        he::EvolveSegment::Risk,
    ];
    let mut ok = he::SEGMENTS.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        if he::SEGMENTS[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("EX810-规格-05-四段封闭表对拍");
    } else {
        set.fail("EX810-规格-05-四段封闭表对拍", "四段漂移或次序错");
    }
}

fn chk_bound_charters_both_sides(set: &mut CheckSet) {
    // 边界-04：登记章程过闸 / 可撤销洗除与空章程双向必拒（0x99C4/0x99C3）。
    let stripped: [&str; 4] = [
        "预测：硬件趋势给区间不押单点",
        "规划：预测到路线的转化",
        "预留：架构面只留接口不留实现",
        "风险：预测落空显性记账",
    ];
    let blank: [&str; 4] = [
        "预测：区间化研判",
        "规划：路线转化",
        "预留：接口预留",
        "",
    ];
    let ok = he::verify_segment_charters(he::SEGMENT_CHARTER).is_ok()
        && matches!(he::verify_segment_charters(stripped), Err(c) if c == 0x99C4)
        && matches!(he::verify_segment_charters(blank), Err(c) if c == 0x99C3);
    if ok {
        set.ok("EX810-边界-04-章程闸两侧钉死");
    } else {
        set.fail("EX810-边界-04-章程闸两侧钉死", "可撤销洗除或空章程漏抓");
    }
}

fn chk_spec_w10_fulfillment(set: &mut CheckSet) {
    // 规格-06：W10 兑现三条写死对拍（挂载点/更新规则/退场规则）。
    let want: [&str; 3] = [
        "章节挂载点：演进观察与预测的文档章节位与更新节奏",
        "更新规则：硬件节点事件触发的章节更新与复述规则",
        "退场规则：预测证伪后的章节归档与预留撤销记录规则",
    ];
    let mut ok = he::W10_FULFILLMENT.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if he::W10_FULFILLMENT[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if !matches!(he::verify_w10_fulfilled(), Ok(r) if r.items == 3 && r.confirmed) {
        ok = false;
    }
    if ok {
        set.ok("EX810-规格-06-兑现三条写死对拍");
    } else {
        set.fail("EX810-规格-06-兑现三条写死对拍", "W10 兑现漂移或空欠");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-05：0x99C 段八码写死互异 + 与前三段（0x9B/0x99A/0x99B）三十六码跨文件互异。
    let codes: [u16; 8] = [
        he::E_X810_MISSING_ITEM,
        he::E_X810_LINK_UNCONFIRMED,
        he::E_X810_POSITIONING_FAULT,
        he::E_X810_SEGMENT_INCOMPLETE,
        he::E_X810_REVERSIBILITY_MISSING,
        he::E_X810_W10_UNFULFILLED,
        he::E_X810_VERSION_MISMATCH,
        E_X810_CHECK_TALLY,
    ];
    let want: [u16; 8] = [0x99C0, 0x99C1, 0x99C2, 0x99C3, 0x99C4, 0x99C5, 0x99C6, 0x99C7];
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
    let others: [u16; 36] = [
        0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07, 0x9B10, 0x9B11,
        0x9B12, 0x9B13, 0x9B14, 0x9B15, 0x99A0, 0x99A1, 0x99A2, 0x99A3, 0x99A4, 0x99A5,
        0x99A6, 0x99A7, 0x99B0, 0x99B1, 0x99B2, 0x99B3, 0x99B4, 0x99B5, 0x99B6, 0x99B8,
        0x99B9, 0x99BA, 0x99BB, 0x99BC, 0x99BD, 0x99BE,
    ];
    for c in codes.iter() {
        let mut k = 0usize;
        while k < 36 {
            if *c == others[k] {
                ok = false;
            }
            k += 1;
        }
    }
    if ok {
        set.ok("EX810-边界-05-八码互异且跨段不撞");
    } else {
        set.fail("EX810-边界-05-八码互异且跨段不撞", "码漂移或跨段撞码");
    }
}

fn chk_bound_domain_ledger(set: &mut CheckSet) {
    // 边界-06：X 域开工总账——号段 (3681,3840)、总项 160（独立重算）。
    let recomputed = 3840 - 3681 + 1;
    let ok = he::X_DOMAIN_RANGE == (3681, 3840)
        && he::x_domain_total() == 160
        && recomputed == 160;
    if ok {
        set.ok("EX810-边界-06-开工总账160");
    } else {
        set.fail("EX810-边界-06-开工总账160", "号段或总账漂移");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_receipt_version(set: &mut CheckSet) {
    // B-01：签收回执版本承载——回执版本与冻结版同源且长度 16（CX81-hwevolve-v1）。
    let all = [true, true, true, true, true, true, true];
    let ok = match he::sign_off(all, true) {
        Ok(r) => r.version.len() == 16 && r.version == he::VCX81_VERSION,
        Err(_) => false,
    };
    if ok {
        set.ok("EX810-承载力-01-回执版本同源");
    } else {
        set.fail("EX810-承载力-01-回执版本同源", "版本承载断裂");
    }
}

fn chk_carry_w10_total(set: &mut CheckSet) {
    // B-02：兑现回执承载力——条目 3 且确认在场。
    let ok = matches!(he::verify_w10_fulfilled(), Ok(r) if r.items == 3 && r.confirmed);
    if ok {
        set.ok("EX810-承载力-02-兑现条目承载");
    } else {
        set.fail("EX810-承载力-02-兑现条目承载", "兑现承载断裂");
    }
}

fn chk_carry_version_both_ways(set: &mut CheckSet) {
    // B-03：版本校验双向——冻结版放行，异版拒收 0x99C6。
    let ok = he::check_version(EXPECT_VERSION).is_ok()
        && matches!(he::check_version("CX81-hwevolve-v2"), Err(c) if c == 0x99C6);
    if ok {
        set.ok("EX810-承载力-03-版本双向校验");
    } else {
        set.fail("EX810-承载力-03-版本双向校验", "版本闸漏抓");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcx81_hwevolve_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EX810-判据-零panic面");
    } else {
        set.fail("EX810-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcx81_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EX810-判据-聚合守恒防自调");
    } else {
        set.fail("EX810-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=四组规格+边界 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：四组规格 + 边界（12 条）。
pub fn run_vcx81_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcx81/a");
    chk_spec_handover_seven(&mut s);
    chk_spec_link_package(&mut s);
    chk_bound_missing_item(&mut s);
    chk_bound_link_unconfirmed(&mut s);
    chk_spec_signoff_receipt(&mut s);
    chk_spec_positioning(&mut s);
    chk_bound_positioning_gate(&mut s);
    chk_spec_segments(&mut s);
    chk_bound_charters_both_sides(&mut s);
    chk_spec_w10_fulfillment(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_domain_ledger(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条）。
pub fn run_vcx81_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcx81/b");
    chk_carry_receipt_version(&mut s);
    chk_carry_w10_total(&mut s);
    chk_carry_version_both_ways(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcx81_checks() -> CheckSet {
    CheckSet::merge(
        run_vcx81_checks_a_standalone(),
        run_vcx81_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcx81_checks_a_standalone, run_vcx81_checks_b_standalone, run_vcx81_checks};

    #[test]
    fn x01_a_standalone_all_green() {
        let set = run_vcx81_checks_a_standalone();
        assert!(set.all_passed(), "vcx81 A 批红项存在");
    }

    #[test]
    fn x01_b_standalone_all_green() {
        let set = run_vcx81_checks_b_standalone();
        assert!(set.all_passed(), "vcx81 B 批红项存在");
    }

    #[test]
    fn x01_merged_all_green() {
        let set = run_vcx81_checks();
        assert!(set.all_passed(), "vcx81 merged 红项存在");
    }
}
