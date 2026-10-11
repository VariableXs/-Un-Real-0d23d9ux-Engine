//! CGPU-F2723 域自检（CGPU-R 域渲染数据信任分级判据层）
//!
//! 判据侧**独立写死**期望（三级名册、六行分级表、原则声明、联动三行、
//! 0x99B 段七码写死互异并与 0x99A/0x9B 段跨文件互异复核）。判据侧自备
//! 独立校验器跑变异表（重名/越级/外部未声明/处置空各向拒绝）与传递闸
//! 双向（升权拒/降权未验证拒/同级免验放行）。聚合防自调：族内判据只调
//! 另一族 standalone + 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcr23_trustlevel as tl;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（R 域 0x99B 段续编，与实现文件六码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_R230_CHECK_TALLY: u16 = 0x99B6;

const EXPECT_VERSION: &str = "CR23-trustlevel-v1";

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
// 判据侧独立校验器（同判据异实现——变异表双向验证用）
// ---------------------------------------------------------------------------

/// 独立分级表校验器（行名唯一 + tier<3 + rule 非空 + 外部级显性不信任）。
fn audit_table(rows: &[tl::TrustRow]) -> Result<usize, u16> {
    if rows.is_empty() {
        return Err(tl::E_R230_TABLE_INCOMPLETE);
    }
    let mut i = 0usize;
    while i < rows.len() {
        let mut j = i + 1;
        while j < rows.len() {
            if rows[i].name == rows[j].name {
                return Err(tl::E_R230_TABLE_INCOMPLETE);
            }
            j += 1;
        }
        i += 1;
    }
    for row in rows.iter() {
        if row.tier >= 3 {
            return Err(tl::E_R230_TIER_UNKNOWN);
        }
        if row.rule.is_empty() {
            return Err(tl::E_R230_TABLE_INCOMPLETE);
        }
        if row.tier == 2 && !row.rule.contains("不信任") {
            return Err(tl::E_R230_TRUST_UNDECLARED);
        }
    }
    Ok(rows.len())
}

/// 独立传递闸校验器（与 tl::verify_transit 同判据异实现）。
fn audit_transit(from: usize, to: usize, validated: bool) -> Result<(), u16> {
    if from >= 3 || to >= 3 {
        return Err(tl::E_R230_TIER_UNKNOWN);
    }
    if from > to {
        return Err(tl::E_R230_TRANSIT_UNVERIFIED);
    }
    if from < to && !validated {
        return Err(tl::E_R230_TRANSIT_UNVERIFIED);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// A 族 · 分级组 + 传递组（规格 + 边界）
// ---------------------------------------------------------------------------

fn chk_spec_tiers(set: &mut CheckSet) {
    // 规格-01：三级名册写死对拍（本域可信/跨域验证/外部不信任，次序即秩）。
    let want: [&str; 3] = ["本域可信", "跨域验证", "外部不信任"];
    let mut ok = tl::TRUST_TIERS.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if tl::TRUST_TIERS[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ER230-规格-01-三级名册写死对拍");
    } else {
        set.fail("ER230-规格-01-三级名册写死对拍", "三级漂移或次序错");
    }
}

fn chk_spec_trust_table(set: &mut CheckSet) {
    // 规格-02：分级表六行、行名写死对拍、tier 分布（可信 2/验证 2/不信任 2）。
    let want_names: [&str; 6] = [
        "内部顶点缓冲", "内部 uniform 集", "跨域纹理", "跨会话缓存", "用户上传资产", "网络拉取资源",
    ];
    let mut ok = tl::TRUST_TABLE.len() == 6;
    let mut t = [0usize; 3];
    let mut i = 0usize;
    while i < 6 {
        if tl::TRUST_TABLE[i].name != want_names[i] {
            ok = false;
        }
        if tl::TRUST_TABLE[i].tier < 3 {
            t[tl::TRUST_TABLE[i].tier] += 1;
        }
        i += 1;
    }
    if t[0] != 2 || t[1] != 2 || t[2] != 2 {
        ok = false;
    }
    if ok {
        set.ok("ER230-规格-02-分级表六行写死对拍");
    } else {
        set.fail("ER230-规格-02-分级表六行写死对拍", "分级表漂移或级分布失衡");
    }
}

fn chk_bound_table_pass(set: &mut CheckSet) {
    // 边界-01：登记表过实现闸 + 过判据独立闸（双闸同绿）。
    let impl_ok = matches!(tl::verify_trust_table(), Ok(n) if n == 6);
    let audit_ok = matches!(audit_table(&tl::TRUST_TABLE), Ok(n) if n == 6);
    if impl_ok && audit_ok {
        set.ok("ER230-边界-01-登记表双闸通过");
    } else {
        set.fail("ER230-边界-01-登记表双闸通过", "登记表被误拒或双闸不同判");
    }
}

fn chk_bound_table_variants(set: &mut CheckSet) {
    // 边界-02：变异表四向必拒（重名/越级/处置空/外部未声明不信任）。
    let mut dup: Vec<tl::TrustRow> = Vec::new();
    for row in tl::TRUST_TABLE.iter() {
        dup.push(row.clone());
    }
    dup[1].name = dup[0].name;
    let over: Vec<tl::TrustRow> = vec![tl::TrustRow {
        tier: 3,
        name: "越级行",
        rule: "n",
    }];
    let mut blank: Vec<tl::TrustRow> = Vec::new();
    for row in tl::TRUST_TABLE.iter() {
        blank.push(row.clone());
    }
    blank[0].rule = "";
    let mut undeclared: Vec<tl::TrustRow> = Vec::new();
    for row in tl::TRUST_TABLE.iter() {
        undeclared.push(row.clone());
    }
    undeclared[4].rule = "外部来源：常规解析";
    let ok = matches!(audit_table(&dup), Err(c) if c == tl::E_R230_TABLE_INCOMPLETE)
        && matches!(audit_table(&over), Err(c) if c == tl::E_R230_TIER_UNKNOWN)
        && matches!(audit_table(&blank), Err(c) if c == tl::E_R230_TABLE_INCOMPLETE)
        && matches!(audit_table(&undeclared), Err(c) if c == tl::E_R230_TRUST_UNDECLARED);
    if ok {
        set.ok("ER230-边界-02-变异表四向必拒");
    } else {
        set.fail("ER230-边界-02-变异表四向必拒", "某向变异漏抓");
    }
}

fn chk_spec_no_transit(set: &mut CheckSet) {
    // 规格-03：原则声明写死对拍（「不传递」为不可删字）。
    let ok = tl::NO_TRANSIT == "信任不传递——跨过边界就要重新验证";
    if ok {
        set.ok("ER230-规格-03-原则声明写死对拍");
    } else {
        set.fail("ER230-规格-03-原则声明写死对拍", "原则声明漂移");
    }
}

fn chk_bound_no_transit_gate(set: &mut CheckSet) {
    // 边界-03：正确声明放行 / 口号式声明失守 0x99B2（反例不含「不传递」）。
    let ok = tl::verify_no_transit("信任不传递——跨过边界就要重新验证").is_ok()
        && matches!(tl::verify_no_transit("信任随时传递——边界只是参考"), Err(c) if c == 0x99B2);
    if ok {
        set.ok("ER230-边界-03-原则闸两侧钉死");
    } else {
        set.fail("ER230-边界-03-原则闸两侧钉死", "口号式声明漏抓");
    }
}

fn chk_spec_transit_gate(set: &mut CheckSet) {
    // 规格-04：传递闸四向（升权拒/降权未验证拒/降权已验证过/同级免验过）。
    let ok = matches!(tl::verify_transit(2, 0, true), Err(c) if c == 0x99B3)
        && matches!(tl::verify_transit(0, 2, false), Err(c) if c == 0x99B3)
        && tl::verify_transit(0, 2, true).is_ok()
        && tl::verify_transit(1, 1, false).is_ok();
    if ok {
        set.ok("ER230-规格-04-传递闸四向钉死");
    } else {
        set.fail("ER230-规格-04-传递闸四向钉死", "传递闸漏抓或误拒");
    }
}

fn chk_bound_transit_variants(set: &mut CheckSet) {
    // 边界-04：独立传递闸同判四向 + 越级拒（from/to>=3 → TIER_UNKNOWN）。
    let ok = matches!(audit_transit(2, 0, true), Err(c) if c == tl::E_R230_TRANSIT_UNVERIFIED)
        && matches!(audit_transit(0, 1, false), Err(c) if c == tl::E_R230_TRANSIT_UNVERIFIED)
        && audit_transit(0, 0, false).is_ok()
        && matches!(audit_transit(0, 3, true), Err(c) if c == tl::E_R230_TIER_UNKNOWN);
    if ok {
        set.ok("ER230-边界-04-独立闸同判且越级拒");
    } else {
        set.fail("ER230-边界-04-独立闸同判且越级拒", "双闸不同判或越级漏抓");
    }
}

fn chk_spec_threat_linkage(set: &mut CheckSet) {
    // 规格-05：联动三行与威胁名目逐行对拍 + verify_threat_linkage 过闸。
    let mut ok = tl::THREAT_LINKAGE.len() == 3;
    if !matches!(tl::verify_threat_linkage(), Ok(3)) {
        ok = false;
    }
    if ok {
        set.ok("ER230-规格-05-威胁联动逐行对拍");
    } else {
        set.fail("ER230-规格-05-威胁联动逐行对拍", "联动声明漂移或缺威胁名目");
    }
}

fn chk_spec_rules_anchor(set: &mut CheckSet) {
    // 规格-06：处置声明结构锚写死对拍——内部行含「直接」，跨域行含「验证」，
    // 外部行含「沙箱解析」（处置结构随信任级单调收紧）。
    let ok = tl::TRUST_TABLE[0].rule.contains("直接")
        && tl::TRUST_TABLE[1].rule.contains("直接")
        && tl::TRUST_TABLE[2].rule.contains("验证")
        && tl::TRUST_TABLE[3].rule.contains("校验")
        && tl::TRUST_TABLE[4].rule.contains("沙箱解析")
        && tl::TRUST_TABLE[5].rule.contains("沙箱解析");
    if ok {
        set.ok("ER230-规格-06-处置结构锚逐行对拍");
    } else {
        set.fail("ER230-规格-06-处置结构锚逐行对拍", "处置结构随级漂移");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-05：0x99B 段七码写死互异 + 与 0x99A/0x9B 段二十二码跨文件互异。
    let codes: [u16; 7] = [
        tl::E_R230_TABLE_INCOMPLETE,
        tl::E_R230_TRUST_UNDECLARED,
        tl::E_R230_TRANSIT_FAULT,
        tl::E_R230_TRANSIT_UNVERIFIED,
        tl::E_R230_TIER_UNKNOWN,
        tl::E_R230_VERSION_MISMATCH,
        E_R230_CHECK_TALLY,
    ];
    let want: [u16; 7] = [0x99B0, 0x99B1, 0x99B2, 0x99B3, 0x99B4, 0x99B5, 0x99B6];
    let mut ok = true;
    let mut i = 0usize;
    while i < 7 {
        if codes[i] != want[i] {
            ok = false;
        }
        let mut j = 0usize;
        while j < 7 {
            if i != j && codes[i] == codes[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let others: [u16; 22] = [
        0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07, 0x9B10, 0x9B11,
        0x9B12, 0x9B13, 0x9B14, 0x9B15, 0x99A0, 0x99A1, 0x99A2, 0x99A3, 0x99A4, 0x99A5,
        0x99A6, 0x99A7,
    ];
    for c in codes.iter() {
        let mut k = 0usize;
        while k < 22 {
            if *c == others[k] {
                ok = false;
            }
            k += 1;
        }
    }
    if ok {
        set.ok("ER230-边界-05-七码互异且跨段不撞");
    } else {
        set.fail("ER230-边界-05-七码互异且跨段不撞", "码漂移或跨段撞码");
    }
}

fn chk_bound_version(set: &mut CheckSet) {
    // 边界-06：版本写死 + 双向校验（冻结版放行/异版拒收）。
    let ok = tl::VCR23_VERSION == EXPECT_VERSION
        && tl::check_version(EXPECT_VERSION).is_ok()
        && matches!(tl::check_version("CR23-trustlevel-v2"), Err(c) if c == tl::E_R230_VERSION_MISMATCH);
    if ok {
        set.ok("ER230-边界-06-版本双向校验");
    } else {
        set.fail("ER230-边界-06-版本双向校验", "版本闸漏抓");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_trust_row(set: &mut CheckSet) {
    // B-01：TrustRow 字段承载——构造+三字段读回一致。
    let row = tl::TrustRow {
        tier: 2,
        name: "判据测试行",
        rule: "外部来源不信任：判据承载测试",
    };
    let ok = row.tier == 2 && row.name == "判据测试行" && row.rule.contains("不信任");
    if ok {
        set.ok("ER230-承载力-01-分级行字段承载");
    } else {
        set.fail("ER230-承载力-01-分级行字段承载", "字段承载断裂");
    }
}

fn chk_carry_counts(set: &mut CheckSet) {
    // B-02：两闸返回数与常量表行数一致（6/3）。
    let t = matches!(tl::verify_trust_table(), Ok(n) if n == tl::TRUST_TABLE.len());
    let l = matches!(tl::verify_threat_linkage(), Ok(n) if n == tl::THREAT_LINKAGE.len());
    if t && l {
        set.ok("ER230-承载力-02-行数守恒");
    } else {
        set.fail("ER230-承载力-02-行数守恒", "行数承载断裂");
    }
}

fn chk_carry_version_len(set: &mut CheckSet) {
    // B-03：版本承载——长度 18（C R 2 3 - t r u s t l e v e l - v 1）且同源。
    let ok = tl::VCR23_VERSION.len() == 18 && tl::VCR23_VERSION == EXPECT_VERSION;
    if ok {
        set.ok("ER230-承载力-03-版本长度承载");
    } else {
        set.fail("ER230-承载力-03-版本长度承载", "版本承载断裂");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcr23_trustlevel_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("ER230-判据-零panic面");
    } else {
        set.fail("ER230-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcr23_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("ER230-判据-聚合守恒防自调");
    } else {
        set.fail("ER230-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=分级组+传递组 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：分级组 + 传递组（12 条：规格 6 + 边界 6）。
pub fn run_vcr23_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcr23/a");
    chk_spec_tiers(&mut s);
    chk_spec_trust_table(&mut s);
    chk_bound_table_pass(&mut s);
    chk_bound_table_variants(&mut s);
    chk_spec_no_transit(&mut s);
    chk_bound_no_transit_gate(&mut s);
    chk_spec_transit_gate(&mut s);
    chk_bound_transit_variants(&mut s);
    chk_spec_threat_linkage(&mut s);
    chk_spec_rules_anchor(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_version(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条）。
pub fn run_vcr23_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcr23/b");
    chk_carry_trust_row(&mut s);
    chk_carry_counts(&mut s);
    chk_carry_version_len(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcr23_checks() -> CheckSet {
    CheckSet::merge(
        run_vcr23_checks_a_standalone(),
        run_vcr23_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcr23_checks_a_standalone, run_vcr23_checks_b_standalone, run_vcr23_checks};

    #[test]
    fn r23_a_standalone_all_green() {
        let set = run_vcr23_checks_a_standalone();
        assert!(set.all_passed(), "vcr23 A 批红项存在");
    }

    #[test]
    fn r23_b_standalone_all_green() {
        let set = run_vcr23_checks_b_standalone();
        assert!(set.all_passed(), "vcr23 B 批红项存在");
    }

    #[test]
    fn r23_merged_all_green() {
        let set = run_vcr23_checks();
        assert!(set.all_passed(), "vcr23 merged 红项存在");
    }
}
