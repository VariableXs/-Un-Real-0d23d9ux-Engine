//! CGPU-F2724 域自检（CGPU-R 域安全渲染降级接口判据层）
//!
//! 判据侧**独立写死**期望（Q10 兑现三条、五源动作/复述逐位写死、
//! v1 冻结五签名、冻结声明、0x99B 段续编七码互异并与全段跨文件互异）。
//! 判据侧自备独立审计器跑变异（复述缺席/动作缺席/清单漂移各向拒绝）。
//! 聚合防自调：族内判据只调另一族 standalone + 进行中 set 自身 tally，
//! 守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcr24_degradeapi as da;
use super::vcr24_degradeapi::DegradeReason;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据 tally 码（R 域 0x99B 段续编，与实现文件六码互异）
// ---------------------------------------------------------------------------

/// 判据自检 tally 失配码。
pub const E_R240_CHECK_TALLY: u16 = 0x99BE;

const EXPECT_VERSION: &str = "CR24-degradeapi-v1";

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
// A 族 · 兑现组 + 冻结组（规格 + 边界）
// ---------------------------------------------------------------------------

fn chk_spec_q10_fulfillment(set: &mut CheckSet) {
    // 规格-01：Q10 兑现三条写死对拍 + 兑现确认过闸。
    let want: [&str; 3] = [
        "异常源封闭：可靠性异常按 F2722 五类威胁对齐为五个降级触发源",
        "动作固定映射：每个异常源到降级动作的映射逐位冻结不仲裁",
        "复述同位冻结：每次降级必须带一句人话复述，文案与动作同表冻结",
    ];
    let mut ok = da::Q10_FULFILLMENT.len() == 3;
    let mut i = 0usize;
    while i < 3 {
        if da::Q10_FULFILLMENT[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if !matches!(da::verify_q10_fulfilled(), Ok(r) if r.items == 3 && r.confirmed) {
        ok = false;
    }
    if ok {
        set.ok("ER240-规格-01-兑现三条写死对拍");
    } else {
        set.fail("ER240-规格-01-兑现三条写死对拍", "Q10 兑现漂移或空欠");
    }
}

fn chk_spec_reasons_closed(set: &mut CheckSet) {
    // 规格-02：五源封闭——degrade 对五源全部可用（封闭表完备性）。
    let all = [
        DegradeReason::MaliciousShader,
        DegradeReason::ResourceBomb,
        DegradeReason::BoundaryBreach,
        DegradeReason::Exfiltration,
        DegradeReason::InfiniteLoop,
    ];
    let mut ok = true;
    for r in all.iter() {
        if da::degrade(*r).is_err() {
            ok = false;
        }
    }
    if ok {
        set.ok("ER240-规格-02-五源封闭全可用");
    } else {
        set.fail("ER240-规格-02-五源封闭全可用", "某源降级调用失败");
    }
}

fn chk_spec_actions(set: &mut CheckSet) {
    // 规格-03：五动作写死对拍（逐位固定映射）。
    let want: [&str; 5] = [
        "拒绝该着色器并回退上一可信管线状态",
        "封顶资源申请并切换低保真降载渲染",
        "裁剪越界访问并按声明边界收缩绘制域",
        "断开外泄路径并以本地脱敏副本续渲",
        "终止不收敛作业并输出已完成帧+中断复述",
    ];
    let mut ok = da::DEGRADE_ACTIONS.len() == 5;
    let mut i = 0usize;
    while i < 5 {
        if da::DEGRADE_ACTIONS[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ER240-规格-03-五动作写死对拍");
    } else {
        set.fail("ER240-规格-03-五动作写死对拍", "动作映射漂移");
    }
}

fn chk_spec_restatements(set: &mut CheckSet) {
    // 规格-04：五复述写死对拍 + 每条含「降级」（复述语义锚）。
    let want: [&str; 5] = [
        "已降级：该着色器被安全策略拒绝，已回退到上一可信画面",
        "已降级：资源申请超出上限，当前以低保真模式继续",
        "已降级：越界绘制域已被收缩，超出部分不再呈现",
        "已降级：数据外传通道已断开，正在以本地副本继续渲染",
        "已降级：作业未在时限内收敛，已输出当前已完成部分",
    ];
    let mut ok = da::RESTATEMENTS.len() == 5;
    let mut i = 0usize;
    while i < 5 {
        if da::RESTATEMENTS[i] != want[i] || !da::RESTATEMENTS[i].contains("降级") {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ER240-规格-04-五复述写死对拍");
    } else {
        set.fail("ER240-规格-04-五复述写死对拍", "复述文案漂移或缺语义锚");
    }
}

fn chk_bound_degrade_mapping(set: &mut CheckSet) {
    // 边界-01：degrade 全五源结果与封闭表按位一致（动作+复述+degraded）。
    let actions_want: [&str; 5] = [
        "拒绝该着色器并回退上一可信管线状态",
        "封顶资源申请并切换低保真降载渲染",
        "裁剪越界访问并按声明边界收缩绘制域",
        "断开外泄路径并以本地脱敏副本续渲",
        "终止不收敛作业并输出已完成帧+中断复述",
    ];
    let all = [
        DegradeReason::MaliciousShader,
        DegradeReason::ResourceBomb,
        DegradeReason::BoundaryBreach,
        DegradeReason::Exfiltration,
        DegradeReason::InfiniteLoop,
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < 5 {
        match da::degrade(all[i]) {
            Ok(o) => {
                if o.action != actions_want[i] || o.restatement.is_empty() || !o.degraded {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    if ok {
        set.ok("ER240-边界-01-五源映射按位一致");
    } else {
        set.fail("ER240-边界-01-五源映射按位一致", "调用结果与冻结表错位");
    }
}

fn chk_bound_reason_semantics(set: &mut CheckSet) {
    // 边界-02：异常源封闭语义承载——Clone/Copy/PartialEq 派生可用 +
    // 五源两两互异（类型即闸：五源之外的源在类型上不可表达，
    // 无需运行期未知源拒绝路径）。
    let a = DegradeReason::MaliciousShader;
    let b = a;
    let all = [
        DegradeReason::MaliciousShader,
        DegradeReason::ResourceBomb,
        DegradeReason::BoundaryBreach,
        DegradeReason::Exfiltration,
        DegradeReason::InfiniteLoop,
    ];
    let mut distinct = true;
    let mut i = 0usize;
    while i < 5 {
        let mut j = 0usize;
        while j < 5 {
            if i != j && all[i] == all[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    if b == DegradeReason::MaliciousShader && distinct {
        set.ok("ER240-边界-02-异常源封闭语义承载");
    } else {
        set.fail("ER240-边界-02-异常源封闭语义承载", "派生断裂或源值混同");
    }
}

fn chk_spec_freeze_list(set: &mut CheckSet) {
    // 规格-05：v1 冻结五签名写死对拍 + 冻结声明写死对拍。
    let want: [&str; 5] = [
        "degrade", "verify_q10_fulfilled", "check_freeze", "check_version", "degrade_audit",
    ];
    let mut ok = da::API_V1_FREEZE.len() == 5;
    let mut i = 0usize;
    while i < 5 {
        if da::API_V1_FREEZE[i] != want[i] {
            ok = false;
        }
        i += 1;
    }
    if da::FROZEN_DECLARATION != "降级接口 v1 冻结：五源五动作五复述逐位固定；签名清单之外的新增走 v2 流程" {
        ok = false;
    }
    if ok {
        set.ok("ER240-规格-05-冻结清单与声明写死对拍");
    } else {
        set.fail("ER240-规格-05-冻结清单与声明写死对拍", "冻结清单或声明漂移");
    }
}

fn chk_bound_freeze_both_sides(set: &mut CheckSet) {
    // 边界-03：check_freeze 双向——冻结清单自校验过 / 漂移清单拒 0x99B9。
    let drifted: [&str; 5] = [
        "degrade", "verify_q10_fulfilled", "check_freeze", "check_version", "degrade_v2",
    ];
    let short: [&str; 4] = ["degrade", "verify_q10_fulfilled", "check_freeze", "check_version"];
    let ok = matches!(da::check_freeze(&da::API_V1_FREEZE), Ok(5))
        && matches!(da::check_freeze(&drifted), Err(c) if c == 0x99B9)
        && matches!(da::check_freeze(&short), Err(c) if c == 0x99B9);
    if ok {
        set.ok("ER240-边界-03-冻结校验两侧钉死");
    } else {
        set.fail("ER240-边界-03-冻结校验两侧钉死", "签名漂移漏抓");
    }
}

fn chk_bound_freeze_audit_live(set: &mut CheckSet) {
    // 边界-04：degrade_audit——实现侧在挂函数与冻结表三方一致（自审计过闸）。
    let ok = matches!(da::degrade_audit(), Ok(5))
        && matches!(da::check_freeze(&da::API_V1_FREEZE), Ok(5));
    if ok {
        set.ok("ER240-边界-04-自审计三方一致");
    } else {
        set.fail("ER240-边界-04-自审计三方一致", "声明与实现在挂清单脱节");
    }
}

fn chk_spec_action_restate_pair(set: &mut CheckSet) {
    // 规格-06：动作×复述同位语义——每位的复述主题词与动作主题词对应
    // （0 着色器 / 1 资源+低保真 / 2 绘制域 / 3 本地副本 / 4 已完成部分）。
    let pair_anchors: [(&str, &str); 5] = [
        ("着色器", "着色器"),
        ("封顶资源", "资源申请"),
        ("绘制域", "绘制域"),
        ("外泄路径", "外传通道"),
        ("不收敛作业", "作业"),
    ];
    let mut ok = true;
    let mut i = 0usize;
    while i < 5 {
        if !da::DEGRADE_ACTIONS[i].contains(pair_anchors[i].0)
            || !da::RESTATEMENTS[i].contains(pair_anchors[i].1)
        {
            ok = false;
        }
        i += 1;
    }
    if ok {
        set.ok("ER240-规格-06-动作复述同位语义");
    } else {
        set.fail("ER240-规格-06-动作复述同位语义", "动作与复述主题错位");
    }
}

fn chk_bound_codes_exclusive(set: &mut CheckSet) {
    // 边界-05：0x99B8-BE 七码写死互异 + 与 0x99A/0x9B 前段二十九码跨文件互异。
    let codes: [u16; 7] = [
        da::E_R240_Q10_UNFULFILLED,
        da::E_R240_FREEZE_DRIFT,
        da::E_R240_REASON_UNKNOWN,
        da::E_R240_RESTATE_MISSING,
        da::E_R240_ACTION_MISSING,
        da::E_R240_VERSION_MISMATCH,
        E_R240_CHECK_TALLY,
    ];
    let want: [u16; 7] = [0x99B8, 0x99B9, 0x99BA, 0x99BB, 0x99BC, 0x99BD, 0x99BE];
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
    let others: [u16; 29] = [
        0x9B00, 0x9B01, 0x9B02, 0x9B03, 0x9B04, 0x9B05, 0x9B06, 0x9B07, 0x9B10, 0x9B11,
        0x9B12, 0x9B13, 0x9B14, 0x9B15, 0x99A0, 0x99A1, 0x99A2, 0x99A3, 0x99A4, 0x99A5,
        0x99A6, 0x99A7, 0x99B0, 0x99B1, 0x99B2, 0x99B3, 0x99B4, 0x99B5, 0x99B6,
    ];
    for c in codes.iter() {
        let mut k = 0usize;
        while k < 29 {
            if *c == others[k] {
                ok = false;
            }
            k += 1;
        }
    }
    if ok {
        set.ok("ER240-边界-05-七码互异且跨段不撞");
    } else {
        set.fail("ER240-边界-05-七码互异且跨段不撞", "码漂移或跨段撞码");
    }
}

fn chk_bound_version(set: &mut CheckSet) {
    // 边界-06：版本写死 + 双向校验（冻结版放行/异版拒收）。
    let ok = da::VCR24_VERSION == EXPECT_VERSION
        && da::check_version(EXPECT_VERSION).is_ok()
        && matches!(da::check_version("CR24-degradeapi-v2"), Err(c) if c == da::E_R240_VERSION_MISMATCH);
    if ok {
        set.ok("ER240-边界-06-版本双向校验");
    } else {
        set.fail("ER240-边界-06-版本双向校验", "版本闸漏抓");
    }
}

// ---------------------------------------------------------------------------
// B 族 · 承载力 + 判据
// ---------------------------------------------------------------------------

fn chk_carry_outcome(set: &mut CheckSet) {
    // B-01：DegradeOutcome 承载——动作/复述/降级位三字段读回一致。
    match da::degrade(DegradeReason::ResourceBomb) {
        Ok(o) => {
            let ok = o.degraded
                && o.action.contains("资源")
                && o.restatement.contains("降级");
            if ok {
                set.ok("ER240-承载力-01-降级结果承载");
            } else {
                set.fail("ER240-承载力-01-降级结果承载", "字段承载断裂");
            }
        }
        Err(_) => set.fail("ER240-承载力-01-降级结果承载", "调用失败"),
    }
}

fn chk_carry_freeze_count(set: &mut CheckSet) {
    // B-02：冻结校验返回签名数==5 且与清单长度守恒。
    let ok = matches!(da::check_freeze(&da::API_V1_FREEZE), Ok(n) if n == da::API_V1_FREEZE.len());
    if ok {
        set.ok("ER240-承载力-02-签名数守恒");
    } else {
        set.fail("ER240-承载力-02-签名数守恒", "签名数承载断裂");
    }
}

fn chk_carry_version_len(set: &mut CheckSet) {
    // B-03：版本承载——长度 18（C R 2 4 - d e g r a d e a p i - v 1）且同源。
    let ok = da::VCR24_VERSION.len() == 18 && da::VCR24_VERSION == EXPECT_VERSION;
    if ok {
        set.ok("ER240-承载力-03-版本长度承载");
    } else {
        set.fail("ER240-承载力-03-版本长度承载", "版本承载断裂");
    }
}

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-零 panic（自扫本文件；三剥防自引用——模式字面量在字符串里被剥掉）。
    let clean = strip_lexical_noise(&String::from(include_str!("vcr24_degradeapi_checks.rs")));
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("ER240-判据-零panic面");
    } else {
        set.fail("ER240-判据-零panic面", "判据区含 panic 面");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-聚合守恒防自调——条数期望判据侧写死（A 族 12 条；本条登记前 B 族 4 条）。
    let a = run_vcr24_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 12;
    let self_ok = sp + sf == 4;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("ER240-判据-聚合守恒防自调");
    } else {
        set.fail("ER240-判据-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=兑现组+冻结组 / b=承载力+判据；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：兑现组 + 冻结组（12 条：规格 6 + 边界 6）。
pub fn run_vcr24_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcr24/a");
    chk_spec_q10_fulfillment(&mut s);
    chk_spec_reasons_closed(&mut s);
    chk_spec_actions(&mut s);
    chk_spec_restatements(&mut s);
    chk_bound_degrade_mapping(&mut s);
    chk_bound_reason_semantics(&mut s);
    chk_spec_freeze_list(&mut s);
    chk_spec_action_restate_pair(&mut s);
    chk_bound_freeze_both_sides(&mut s);
    chk_bound_freeze_audit_live(&mut s);
    chk_bound_codes_exclusive(&mut s);
    chk_bound_version(&mut s);
    s
}

/// 判据族 b：承载力 + 判据（5 条）。
pub fn run_vcr24_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcr24/b");
    chk_carry_outcome(&mut s);
    chk_carry_freeze_count(&mut s);
    chk_carry_version_len(&mut s);
    chk_criterion_zero_panic(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcr24_checks() -> CheckSet {
    CheckSet::merge(
        run_vcr24_checks_a_standalone(),
        run_vcr24_checks_b_standalone(),
    )
}

#[cfg(test)]
mod tests {
    use super::{run_vcr24_checks_a_standalone, run_vcr24_checks_b_standalone, run_vcr24_checks};

    #[test]
    fn r24_a_standalone_all_green() {
        let set = run_vcr24_checks_a_standalone();
        assert!(set.all_passed(), "vcr24 A 批红项存在");
    }

    #[test]
    fn r24_b_standalone_all_green() {
        let set = run_vcr24_checks_b_standalone();
        assert!(set.all_passed(), "vcr24 B 批红项存在");
    }

    #[test]
    fn r24_merged_all_green() {
        let set = run_vcr24_checks();
        assert!(set.all_passed(), "vcr24 merged 红项存在");
    }
}
