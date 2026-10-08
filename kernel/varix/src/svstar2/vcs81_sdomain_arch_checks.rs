//! CGPU-F2881 域自检（CGPU-S 域开工与多用户总架构判据层）
//!
//! 判据侧**独立写死**期望（移交包七件、衔接包两件、定位声明、公平铁律、
//! 四段封闭表与章程、威胁模型三条、配额建议三条、0x9B 段八码写死互异）。
//! 聚合防自调：族内判据只调另一族 standalone + 进行中 set 自身 tally，
//! 守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcs81_sdomain_arch as sa;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_VERSION: &str = "CS81-sdomain-v1";

/// 单遍词法剥除（行注释+块注释+字符串字面量全剥——panic 词扫描防自引用；
/// 字节级收集保持 UTF-8 完整）。
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

/// 读本文件源码做零 panic 面同源性对拍（include_str! 编译期嵌入真源码）。
fn self_source() -> String {
    String::from(include_str!("vcs81_sdomain_arch_checks.rs"))
}

// ---------------------------------------------------------------------------
// A 族 · 规格 + 边界
// ---------------------------------------------------------------------------

fn chk_spec_handover_seven(set: &mut CheckSet) {
    // 规格-01：移交包七件与写死名单逐项相等（封闭清单对拍）。
    let want: [&str; 7] = [
        "接口冻结", "契约", "资产", "基线", "遗留", "多用户安全衔接包", "教训十条",
    ];
    let mut ok = sa::HANDOVER_SEVEN.len() == 7;
    let mut i = 0usize;
    while i < 7 {
        if sa::HANDOVER_SEVEN[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES810-规格-01-移交包七件写死对拍");
    } else {
        set.fail("ES810-规格-01-移交包七件写死对拍", "七件名册漂移或缺件");
    }
}

fn chk_spec_link_package(set: &mut CheckSet) {
    // 规格-02：衔接包两件名目写死对拍（兑现确认的对照基准）。
    let ok = sa::LINK_PACKAGE.len() == 2
        && sa::LINK_PACKAGE[0] == "每用户威胁模型"
        && sa::LINK_PACKAGE[1] == "每用户配额建议";
    if ok {
        set.ok("ES810-规格-02-衔接包两件写死对拍");
    } else {
        set.fail("ES810-规格-02-衔接包两件写死对拍", "衔接包名目漂移");
    }
}

fn chk_bound_missing_item(set: &mut CheckSet) {
    // 边界-01：缺一件（位 3 遗留 / 位 5 衔接包各自缺席）→ 拒收 0x9B00。
    let hole_a = [true, true, true, false, true, true, true];
    let hole_b = [true, true, true, true, true, false, true];
    let ok = matches!(sa::sign_off(hole_a, true), Err(c) if c == 0x9B00)
        && matches!(sa::sign_off(hole_b, true), Err(c) if c == 0x9B00);
    if ok {
        set.ok("ES810-边界-01-缺件拒收两侧钉死");
    } else {
        set.fail("ES810-边界-01-缺件拒收两侧钉死", "缺件闸漏抓");
    }
}

fn chk_bound_link_unconfirmed(set: &mut CheckSet) {
    // 边界-02：七件齐但衔接确认缺席 → 0x9B01。
    let all = [true, true, true, true, true, true, true];
    let ok = matches!(sa::sign_off(all, false), Err(c) if c == 0x9B01);
    if ok {
        set.ok("ES810-边界-02-衔接未确认拒收");
    } else {
        set.fail("ES810-边界-02-衔接未确认拒收", "衔接确认闸漏抓");
    }
}

fn chk_spec_signoff_receipt(set: &mut CheckSet) {
    // 规格-03：全齐+衔接确认 → 收讫，回执三字段齐备（收 7/确认/版本）。
    let all = [true, true, true, true, true, true, true];
    let ok = match sa::sign_off(all, true) {
        Ok(r) => {
            r.items_signed == 7 && r.link_confirmed && r.version == EXPECT_VERSION
        }
        Err(_) => false,
    };
    if ok {
        set.ok("ES810-规格-03-全齐收讫回执齐备");
    } else {
        set.fail("ES810-规格-03-全齐收讫回执齐备", "回执字段漂移");
    }
}

fn chk_spec_positioning(set: &mut CheckSet) {
    // 规格-04：定位声明与公平铁律写死对拍（「公平」为不可删字）。
    let ok = sa::POSITIONING == "一机多租·公平共享 GPU"
        && sa::FAIRNESS_IRONLAW == "用户间不互窃——公平铁律的会话版";
    if ok {
        set.ok("ES810-规格-04-定位与铁律写死对拍");
    } else {
        set.fail("ES810-规格-04-定位与铁律写死对拍", "定位声明漂移或丢公平");
    }
}

fn chk_bound_positioning_gate(set: &mut CheckSet) {
    // 边界-03：公平声明放行 / 独占声明失守 0x9B02（两侧钉死）。
    let ok = sa::verify_positioning("一机多租·公平共享 GPU").is_ok()
        && matches!(sa::verify_positioning("一机多租·独占 GPU"), Err(c) if c == 0x9B02);
    if ok {
        set.ok("ES810-边界-03-定位校验两侧钉死");
    } else {
        set.fail("ES810-边界-03-定位校验两侧钉死", "独占声明漏抓");
    }
}

fn chk_spec_segments(set: &mut CheckSet) {
    // 规格-05：四段封闭表逐值对拍（模型/隔离/调度/配额，次序固定）。
    let want = [
        sa::Segment::Model,
        sa::Segment::Isolation,
        sa::Segment::Scheduling,
        sa::Segment::Quota,
    ];
    let mut ok = sa::SEGMENTS.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        if sa::SEGMENTS[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES810-规格-05-四段封闭表对拍");
    } else {
        set.fail("ES810-规格-05-四段封闭表对拍", "四段漂移或次序错");
    }
}

fn chk_spec_charters(set: &mut CheckSet) {
    // 规格-06：四段章程写死对拍（隔离段居位 1——铁律先于调度配额声明）。
    let want: [&str; 4] = [
        "模型：多用户会话与角色的承载结构",
        "隔离：用户间不互窃——公平铁律的会话版",
        "调度：多会话公平共享 GPU 时隙",
        "配额：每用户预算与用量的秩序",
    ];
    let mut ok = sa::SEGMENT_CHARTER.len() == 4;
    let mut i = 0usize;
    while i < 4 {
        if sa::SEGMENT_CHARTER[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES810-规格-06-四段章程写死对拍");
    } else {
        set.fail("ES810-规格-06-四段章程写死对拍", "章程漂移");
    }
}

fn chk_bound_charters_gate_pass(set: &mut CheckSet) {
    // 边界-04：登记章程全量过闸。
    let ok = sa::verify_four_segment_charters(sa::SEGMENT_CHARTER).is_ok();
    if ok {
        set.ok("ES810-边界-04-登记章程过闸");
    } else {
        set.fail("ES810-边界-04-登记章程过闸", "合法章程被误拒");
    }
}

fn chk_bound_fairness_stripped(set: &mut CheckSet) {
    // 边界-05：隔离段章程洗掉「不互窃」→ 0x9B04（铁律缺席必被抓）。
    let stripped: [&str; 4] = [
        "模型：多用户会话与角色的承载结构",
        "隔离：用户间互不干扰",
        "调度：多会话公平共享 GPU 时隙",
        "配额：每用户预算与用量的秩序",
    ];
    let ok = matches!(sa::verify_four_segment_charters(stripped), Err(c) if c == 0x9B04);
    if ok {
        set.ok("ES810-边界-05-铁律洗除必抓");
    } else {
        set.fail("ES810-边界-05-铁律洗除必抓", "公平闸漏抓");
    }
}

fn chk_bound_charter_blank(set: &mut CheckSet) {
    // 边界-06：任一段章程空 → 0x9B03（残缺必被抓）。
    let blank: [&str; 4] = ["", "隔离：用户间不互窃", "调度：公平时隙", "配额：预算秩序"];
    let ok = matches!(sa::verify_four_segment_charters(blank), Err(c) if c == 0x9B03);
    if ok {
        set.ok("ES810-边界-06-空章程必抓");
    } else {
        set.fail("ES810-边界-06-空章程必抓", "残缺闸漏抓");
    }
}

fn chk_spec_threat_model(set: &mut CheckSet) {
    // 规格-07：威胁模型三条写死对拍（越权/窃配额/侧信道）。
    let want: [&str; 3] = [
        "越权会话：用户 A 不得触及用户 B 的渲染上下文",
        "配额窃取：用户不得透支他人预算",
        "侧信道：跨用户时序观测必须显性披露",
    ];
    let mut ok = sa::THREAT_MODEL.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if sa::THREAT_MODEL[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES810-规格-07-威胁模型写死对拍");
    } else {
        set.fail("ES810-规格-07-威胁模型写死对拍", "威胁模型漂移");
    }
}

fn chk_spec_quota_advice(set: &mut CheckSet) {
    // 规格-08：配额建议三条写死对拍（保底/弹性/超额显性）。
    let want: [&str; 3] = [
        "基础份额：每用户保底 1/N 算力时隙",
        "弹性上限：空闲回收，忙时不满发",
        "超额显性：超限请求拒绝并复述原因",
    ];
    let mut ok = sa::QUOTA_ADVICE.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if sa::QUOTA_ADVICE[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ES810-规格-08-配额建议写死对拍");
    } else {
        set.fail("ES810-规格-08-配额建议写死对拍", "配额建议漂移");
    }
}

fn chk_spec_r10_receipt(set: &mut CheckSet) {
    // 规格-09：兑现确认 → 回执 threats==3、quotas==3、confirmed。
    let ok = match sa::verify_r10_fulfilled() {
        Ok(f) => f.threats == 3 && f.quotas == 3 && f.confirmed,
        Err(_) => false,
    };
    if ok {
        set.ok("ES810-规格-09-兑现回执齐备");
    } else {
        set.fail("ES810-规格-09-兑现回执齐备", "R10 兑现空欠");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-07：S 域八码写死值逐一对拍 + 两两互异（!= 防自判死）。
    let codes: [u16; 8] = [
        sa::E_S810_MISSING_ITEM,
        sa::E_S810_LINK_UNCONFIRMED,
        sa::E_S810_POSITIONING_FAULT,
        sa::E_S810_SEGMENT_INCOMPLETE,
        sa::E_S810_FAIRNESS_MISSING,
        sa::E_S810_R10_UNFULFILLED,
        sa::E_S810_VERSION_MISMATCH,
        super::vcs81_sdomain_arch_checks::E_S810_CHECK_TALLY,
    ];
    let want: [u16; 8] = [0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07];
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
    if ok {
        set.ok("ES810-边界-07-八码写死互异");
    } else {
        set.fail("ES810-边界-07-八码写死互异", "码漂移或撞码");
    }
}

fn chk_bound_domain_ledger(set: &mut CheckSet) {
    // 边界-08：S 域开工总账——号段 (2881,3040)、总项 160（独立重算）。
    let recomputed = 3040 - 2881 + 1;
    let ok = sa::S_DOMAIN_RANGE == (2881, 3040)
        && sa::s_domain_total() == 160
        && recomputed == 160;
    if ok {
        set.ok("ES810-边界-08-开工总账160");
    } else {
        set.fail("ES810-边界-08-开工总账160", "号段或总账漂移");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 判据承载力
// ---------------------------------------------------------------------------

fn chk_carry_receipt_version(set: &mut CheckSet) {
    // B-01：签收回执版本承载——回执版本与冻结版同源且长度 15。
    let all = [true, true, true, true, true, true, true];
    let ok = match sa::sign_off(all, true) {
        Ok(r) => r.version.len() == 15 && r.version == sa::VCS81_VERSION,
        Err(_) => false,
    };
    if ok {
        set.ok("ES810-承载力-01-回执版本同源");
    } else {
        set.fail("ES810-承载力-01-回执版本同源", "版本承载断裂");
    }
}

fn chk_carry_r10_total(set: &mut CheckSet) {
    // B-02：兑现回执承载力——两件条目合计 6 且确认在场。
    let ok = match sa::verify_r10_fulfilled() {
        Ok(f) => f.threats + f.quotas == 6 && f.confirmed,
        Err(_) => false,
    };
    if ok {
        set.ok("ES810-承载力-02-兑现条目合计6");
    } else {
        set.fail("ES810-承载力-02-兑现条目合计6", "兑现承载断裂");
    }
}

fn chk_carry_version_both_ways(set: &mut CheckSet) {
    // B-03：版本校验双向——冻结版放行，异版拒收 0x9B06。
    let ok = sa::check_version(EXPECT_VERSION).is_ok()
        && matches!(sa::check_version("CS81-sdomain-v2"), Err(c) if c == 0x9B06);
    if ok {
        set.ok("ES810-承载力-03-版本双向校验");
    } else {
        set.fail("ES810-承载力-03-版本双向校验", "版本闸漏抓");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-04 前置：判据面零 panic（自扫本文件；词法剥除防自引用——
    // 模式字面量在字符串里会被剥掉，不命中自身）。
    let clean = strip_lexical_noise(&self_source());
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("ES810-判据-零panic面");
    } else {
        set.fail("ES810-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-04：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 17 条；判据-04 登记前 B 族进行中 4 条）。
    let a = run_vcs81_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 17;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("ES810-判据-聚合守恒防自调");
    } else {
        set.fail("ES810-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界（17 条）。
pub fn run_vcs81_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcs81/a");
    chk_spec_handover_seven(&mut s);
    chk_spec_link_package(&mut s);
    chk_bound_missing_item(&mut s);
    chk_bound_link_unconfirmed(&mut s);
    chk_spec_signoff_receipt(&mut s);
    chk_spec_positioning(&mut s);
    chk_bound_positioning_gate(&mut s);
    chk_spec_segments(&mut s);
    chk_spec_charters(&mut s);
    chk_bound_charters_gate_pass(&mut s);
    chk_bound_fairness_stripped(&mut s);
    chk_bound_charter_blank(&mut s);
    chk_spec_threat_model(&mut s);
    chk_spec_quota_advice(&mut s);
    chk_spec_r10_receipt(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_domain_ledger(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条：承载力 3 + 零 panic 面 + 聚合守恒防自调）。
pub fn run_vcs81_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcs81/b");
    chk_carry_receipt_version(&mut s);
    chk_carry_r10_total(&mut s);
    chk_carry_version_both_ways(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcs81_checks() -> CheckSet {
    CheckSet::merge(
        run_vcs81_checks_a_standalone(),
        run_vcs81_checks_b_standalone(),
    )
}

/// 判据 tally 码（S 域 0x9B 段续编，与实现文件七码互异）。
pub const E_S810_CHECK_TALLY: u16 = 0x9B07;

#[cfg(test)]
mod tests {
    use super::{run_vcs81_checks_a_standalone, run_vcs81_checks_b_standalone, run_vcs81_checks};

    #[test]
    fn s01_a_standalone_all_green() {
        let set = run_vcs81_checks_a_standalone();
        assert!(set.all_passed(), "vcs81 A 批红项存在");
    }

    #[test]
    fn s01_b_standalone_all_green() {
        let set = run_vcs81_checks_b_standalone();
        assert!(set.all_passed(), "vcs81 B 批红项存在");
    }

    #[test]
    fn s01_merged_all_green() {
        let set = run_vcs81_checks();
        assert!(set.all_passed(), "vcs81 merged 红项存在");
    }
}
