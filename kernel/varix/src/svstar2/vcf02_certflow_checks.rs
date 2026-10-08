//! CGPU-F0802 域自检（判据逐条映射：六步/自动化/中断恢复/审计/判据）
//!
//! 判据侧独立重算步序表、标签闭集、审计链（ref_chain 手写混合）——同源恒绿的
//! 弱门禁比没有门禁更坏。聚合防自调：A/B 两族 + tally 守恒。

use crate::checks::CheckSet;

use super::vcf01_gpucompat::{certify, CertEvidence, CertStatus, C_VCF01_UNVERIFIED};
use super::vcf02_certflow as api;
use super::vcf02_certflow::*;

use alloc::string::String;

/// A 族条数（判据侧写死）。
const EXPECT_A_COUNT: usize = 13;
/// B 族防自调 tally 时刻已登记条数（判据侧写死；B 全族 6 条）。
const EXPECT_B_BEFORE: usize = 4;

// ---------------------------------------------------------------------------
// 夹具执行器
// ---------------------------------------------------------------------------

/// stub 执行器：每步成功，凭据指纹确定非零。
struct StubEx;
impl FlowExecutor for StubEx {
    fn run_step(&mut self, step: usize, _tag: &[u8]) -> Result<u64, u16> {
        Ok(fnv64(STEP_NAMES[step].as_bytes()) | 1)
    }
}

/// 抖动执行器：指定步前 `fail_times` 次失败（中断恢复语料的注入点）。
struct FlakyEx {
    fail_step: usize,
    fail_times: u8,
}
impl FlowExecutor for FlakyEx {
    fn run_step(&mut self, step: usize, _tag: &[u8]) -> Result<u64, u16> {
        if step == self.fail_step && self.fail_times > 0 {
            self.fail_times -= 1;
            return Err(C_VCF02_EXEC_FAULT);
        }
        Ok((fnv64(STEP_NAMES[step].as_bytes()) ^ 0x51) | 1)
    }
}

/// 恒败执行器：重跑耗尽语料。
struct AlwaysFailEx;
impl FlowExecutor for AlwaysFailEx {
    fn run_step(&mut self, _step: usize, _tag: &[u8]) -> Result<u64, u16> {
        Err(C_VCF02_EXEC_FAULT)
    }
}

// ---------------------------------------------------------------------------
// A 族：规格 + 自动化 + 恢复 + 铁律 + 审计
// ---------------------------------------------------------------------------

fn chk_steps(set: &mut CheckSet) {
    // 规格-01 六步封闭：表长 6、枚举下标全覆盖互异、步名非空。
    let idx = [
        step_index(CertStep::Apply),
        step_index(CertStep::Probe),
        step_index(CertStep::Bench),
        step_index(CertStep::Judge),
        step_index(CertStep::Grant),
        step_index(CertStep::Publish),
    ];
    let distinct = idx[0] != idx[1]
        && idx[1] != idx[2]
        && idx[2] != idx[3]
        && idx[3] != idx[4]
        && idx[4] != idx[5];
    let mut names_ok = true;
    for n in STEP_NAMES.iter() {
        if n.is_empty() {
            names_ok = false;
        }
    }
    if STEP_COUNT == 6 && STEP_NAMES.len() == 6 && distinct && names_ok {
        set.ok("E802-规格-01-六步封闭");
    } else {
        set.fail("E802-规格-01-六步封闭", "步数/枚举映射/步名漂移");
    }
    // 规格-02 步序固化：依赖表逐位独立重算（i 的前置=i-1，首步无前置）+
    // 行为闸——前置未完成的目标步被拒（跳步在行为层不可达）。
    let mut deps_ok = true;
    let mut di = 0usize;
    while di < STEP_COUNT {
        let want = if di == 0 { 0 } else { di - 1 };
        if STEP_DEPS[di] != want {
            deps_ok = false;
        }
        di += 1;
    }
    let mut flow = CertFlow::new(1, 0x8086, 101);
    let mut stub = StubEx;
    let skip = flow.run_step_at(&mut stub, step_index(CertStep::Bench), TAG_SUITE_RUN);
    if deps_ok && skip == Err(C_VCF02_STEP_ORDER) {
        set.ok("E802-规格-02-步序固化闸");
    } else {
        set.fail("E802-规格-02-步序固化闸", "依赖表漂移或跳步未被拒绝");
    }
    // 规格-03 判定标签封闭三值：三标签各有映射、越集 None。
    let s = tag_to_evidence(9, 9, TAG_SUITE_RUN);
    let c = tag_to_evidence(9, 9, TAG_CLAIM_ONLY);
    let u = tag_to_evidence(9, 9, TAG_UNTESTED);
    let bogus = tag_to_evidence(9, 9, b"BOGUS-TAG");
    let closed_ok = match (s, c, u, bogus) {
        (
            Some(a),
            Some(b),
            Some(d),
            None,
        ) => {
            a.suite_run && !a.doc_claim_only && b.doc_claim_only && !d.suite_run && !d.doc_claim_only
        }
        _ => false,
    };
    if closed_ok {
        set.ok("E802-规格-03-判定标签闭集");
    } else {
        set.fail("E802-规格-03-判定标签闭集", "标签映射或闭集漂移");
    }
    // 规格-04 输入契约固化：判定步标签越出闭集 → 0x9F11。
    let mut flow = CertFlow::new(2, 0x8086, 101);
    let mut stub = StubEx;
    let mut ok3 = true;
    for _ in 0..3 {
        if flow.run(&mut stub, TAG_SUITE_RUN).is_err() {
            ok3 = false;
        }
    }
    let bad_in = flow.run(&mut stub, b"BOGUS-TAG");
    if ok3 && bad_in == Err(C_VCF02_STEP_INPUT) {
        set.ok("E802-规格-04-输入契约闸");
    } else {
        set.fail("E802-规格-04-输入契约闸", "越集标签或坏输入未被拒");
    }
}

fn chk_auto(set: &mut CheckSet) {
    // 自动化-01 端到端无人值守：stub 执行器六步跑通，授予 Certified。
    let mut flow = CertFlow::new(3, 0x8086, 101);
    let mut stub = StubEx;
    let e2e = flow.run_all(&mut stub, TAG_SUITE_RUN);
    if e2e == Ok(CertStatus::Certified) && flow.publish_ready() {
        set.ok("E802-自动化-01-端到端跑通");
    } else {
        set.fail("E802-自动化-01-端到端跑通", "全流程未达 Certified/发布未就绪");
    }
    // 自动化-02 步凭据固化：六步凭据指纹全非零、留痕恰 6 条、审计链完好。
    let mut ev_ok = true;
    let mut i = 0usize;
    while i < STEP_COUNT {
        if flow.evidence_of(i) == 0 {
            ev_ok = false;
        }
        i += 1;
    }
    if ev_ok && flow.audit_len() == 6 && flow.audit_verify().is_ok() {
        set.ok("E802-自动化-02-步凭据与留痕");
    } else {
        set.fail("E802-自动化-02-步凭据与留痕", "凭据为零/留痕条数漂移/链断");
    }
}

fn chk_resume(set: &mut CheckSet) {
    // 恢复-01 失败留痕续跑：基准步抖两败后第三次成功，resume 收敛 Certified。
    let mut flow = CertFlow::new(4, 0x8086, 101);
    let mut stub = StubEx;
    let mut head_ok = true;
    for _ in 0..2 {
        if flow.run(&mut stub, TAG_SUITE_RUN).is_err() {
            head_ok = false;
        }
    }
    let mut flaky = FlakyEx { fail_step: step_index(CertStep::Bench), fail_times: 2 };
    let resumed = flow.resume(&mut flaky, TAG_SUITE_RUN);
    if head_ok
        && resumed == Ok(CertStatus::Certified)
        && flow.attempts_of(step_index(CertStep::Bench)) == 3
        && flow.audit_len() == 8
    {
        set.ok("E802-恢复-01-失败续跑收敛");
    } else {
        set.fail("E802-恢复-01-失败续跑收敛", "续跑未收敛或重试计账漂移");
    }
    // 恢复-02 续跑不重做：已完成步凭据指纹逐位不变、尝试次数不增。
    let ev0 = flow.evidence_of(step_index(CertStep::Apply));
    let ev1 = flow.evidence_of(step_index(CertStep::Probe));
    let a0 = flow.attempts_of(step_index(CertStep::Apply));
    let a1 = flow.attempts_of(step_index(CertStep::Probe));
    if ev0 != 0
        && ev1 != 0
        && flow.evidence_of(step_index(CertStep::Apply)) == ev0
        && flow.evidence_of(step_index(CertStep::Probe)) == ev1
        && flow.attempts_of(step_index(CertStep::Apply)) == a0
        && flow.attempts_of(step_index(CertStep::Probe)) == a1
        && a0 == 1
        && a1 == 1
    {
        set.ok("E802-恢复-02-续跑不重做");
    } else {
        set.fail("E802-恢复-02-续跑不重做", "已完成步被重跑（凭据/次数漂移）");
    }
    // 恢复-03 重跑耗尽上抛：恒败执行器 → 0x9F13，且耗尽前每败留痕。
    let mut flow = CertFlow::new(5, 0x8086, 101);
    let mut doom = AlwaysFailEx;
    let exhausted = flow.resume(&mut doom, TAG_SUITE_RUN);
    let traced = flow.audit_len() == MAX_ATTEMPTS as usize;
    if exhausted == Err(C_VCF02_RESUME_EXHAUSTED) && traced {
        set.ok("E802-恢复-03-重跑耗尽上抛");
    } else {
        set.fail("E802-恢复-03-重跑耗尽上抛", "耗尽未上抛或失败未留痕");
    }
}

fn chk_ironlaw(set: &mut CheckSet) {
    // 铁律-01 文档声明进不了流程版认证：CLAIM-ONLY → 0x9F12，审计留痕 0x9F00。
    let mut flow = CertFlow::new(6, 0x8086, 101);
    let mut stub = StubEx;
    let claimed = flow.run_all(&mut stub, TAG_CLAIM_ONLY);
    let mut seen_root = false;
    let mut i = 0usize;
    while i < flow.audit_len() {
        if let Some(r) = flow.audit_record(i) {
            if r.verdict == C_VCF01_UNVERIFIED {
                seen_root = true;
            }
        }
        i += 1;
    }
    if claimed == Err(C_VCF02_JUDGE_REJECT) && seen_root && !flow.publish_ready() {
        set.ok("E802-铁律-01-文档声明拒认证");
    } else {
        set.fail("E802-铁律-01-文档声明拒认证", "纸面凭据被放行或根因未留痕");
    }
    // 铁律-02 未实测显性状态：UNTESTED → Uncertified 关账，发布不受理，再推进被拒。
    let mut flow = CertFlow::new(7, 0x8086, 101);
    let mut stub = StubEx;
    let untested = flow.run_all(&mut stub, TAG_UNTESTED);
    let closed = flow.run(&mut stub, TAG_SUITE_RUN) == Err(C_VCF02_FLOW_STATE);
    if untested == Ok(CertStatus::Uncertified)
        && flow.granted() == Some(CertStatus::Uncertified)
        && !flow.publish_ready()
        && closed
    {
        set.ok("E802-铁律-02-未实测显性关账");
    } else {
        set.fail("E802-铁律-02-未实测显性关账", "未实测被静默放行或关账语义漂移");
    }
    // 铁律-03 判定同源对拍：流程授予与 certify 直调同判据同判。
    let ev = CertEvidence { device: 0x8086, driver: 101, suite_run: true, doc_claim_only: false };
    let mut flow = CertFlow::new(8, 0x8086, 101);
    let mut stub = StubEx;
    let e2e = flow.run_all(&mut stub, TAG_SUITE_RUN);
    let agree = match (certify(&ev), e2e) {
        (Ok(CertStatus::Certified), Ok(CertStatus::Certified)) => true,
        _ => false,
    };
    if agree && flow.granted() == Some(CertStatus::Certified) {
        set.ok("E802-铁律-03-判定同源对拍");
    } else {
        set.fail("E802-铁律-03-判定同源对拍", "流程判定与 certify 直调失判");
    }
}

fn chk_audit(set: &mut CheckSet) {
    // 审计-01 全流程留痕可回溯：抖动流程逐记录——步号非降、败因留痕、
    // 成功记录凭据非零（判据「每份认证的全流程留痕——可回溯」）。
    let mut flow = CertFlow::new(9, 0x8086, 101);
    let mut stub = StubEx;
    for _ in 0..2 {
        let _ = flow.run(&mut stub, TAG_SUITE_RUN);
    }
    let mut flaky = FlakyEx { fail_step: step_index(CertStep::Bench), fail_times: 1 };
    let _ = flow.resume(&mut flaky, TAG_SUITE_RUN);
    let mut prev_step = 0u8;
    let mut walk_ok = true;
    let mut i = 0usize;
    while i < flow.audit_len() {
        if let Some(r) = flow.audit_record(i) {
            if r.step < prev_step {
                walk_ok = false;
            }
            prev_step = r.step;
            if r.verdict == 0 && r.output_fp == 0 {
                walk_ok = false; // 成功记录必有凭据
            }
            if r.verdict == C_VCF02_EXEC_FAULT && r.output_fp != 0 {
                walk_ok = false; // 失败记录必无凭据
            }
        } else {
            walk_ok = false;
        }
        i += 1;
    }
    if walk_ok && flow.audit_verify().is_ok() {
        set.ok("E802-审计-01-留痕可回溯");
    } else {
        set.fail("E802-审计-01-留痕可回溯", "留痕序/凭据/败因账不平");
    }
}

// ---------------------------------------------------------------------------
// B 族：判据承载力
// ---------------------------------------------------------------------------

fn chk_codes(set: &mut CheckSet) {
    // 判据-01 七码独占 0x9F1x 段、互异、与 vcf01 段（0x9F00-0x9F02）零重叠。
    let codes = [
        C_VCF02_STEP_ORDER,
        C_VCF02_STEP_INPUT,
        C_VCF02_JUDGE_REJECT,
        C_VCF02_RESUME_EXHAUSTED,
        C_VCF02_AUDIT_BROKEN,
        C_VCF02_FLOW_STATE,
        C_VCF02_EXEC_FAULT,
    ];
    let mut distinct = true;
    let mut i = 0usize;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    let seg_ok = codes.iter().all(|c| (c >> 8) == 0x9F);
    let vcf01_max = 0x9F02u16;
    let no_clash = codes.iter().all(|c| *c > vcf01_max);
    if distinct && seg_ok && no_clash {
        set.ok("E802-判据-01-码段独占");
    } else {
        set.fail("E802-判据-01-码段独占", "码漂移/撞段/与 vcf01 重叠");
    }
}

/// 判据侧手写链混合（双源同构——不复用 audit_chain 代码路径）。
fn ref_chain(prev: u64, step: u64, attempt: u64, ifp: u64, ofp: u64, verdict: u64) -> u64 {
    let mut h = prev;
    let fields = [step, attempt, ifp, ofp, verdict];
    let mut i = 0usize;
    while i < 5 {
        h ^= fields[i];
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h ^ prev
}

fn chk_chain(set: &mut CheckSet) {
    // 判据-02 审计链双源对账 + 篡改可检：ref_chain 与 audit_chain 同值，
    // 合法链过审，任一字段被篡改即 0x9F14 断链。
    let rec = AuditRecord {
        step: 3,
        attempt: 2,
        input_fp: 0xA1B2_C3D4_5E6F_0789,
        output_fp: 0,
        verdict: C_VCF02_EXEC_FAULT,
        chain: 0,
    };
    let expect = ref_chain(
        AUDIT_CHAIN_SEED,
        rec.step as u64,
        rec.attempt as u64,
        rec.input_fp,
        rec.output_fp,
        rec.verdict as u64,
    );
    let mut r1 = rec;
    r1.chain = audit_chain(AUDIT_CHAIN_SEED, &rec);
    let legit_ok = expect == r1.chain && audit_verify_records(&[r1]).is_ok();
    // 篡改：输出凭据被翻一位 → 重算不符。
    let mut r2 = r1;
    r2.output_fp ^= 1;
    let tampered = audit_verify_records(&[r2]);
    // 字段合法性：步号 0 / 尝试 0 均拒。
    let mut r3 = r1;
    r3.step = 0;
    let bad_field = audit_verify_records(&[r3]);
    if legit_ok
        && tampered == Err(C_VCF02_AUDIT_BROKEN)
        && bad_field == Err(C_VCF02_AUDIT_BROKEN)
        && AUDIT_CHAIN_SEED != 0
    {
        set.ok("E802-判据-02-审计链双源与篡改可检");
    } else {
        set.fail("E802-判据-02-审计链双源与篡改可检", "链重算失配或篡改未检出");
    }
}

/// 单遍词法剥除（字符串/注释不误伤）。
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

fn chk_zero_panic(set: &mut CheckSet) {
    // 判据-03 生产面零 panic。
    let src = include_str!("vcf02_certflow.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E802-判据-03-生产面零 panic");
    } else {
        set.fail("E802-判据-03-生产面零 panic", "生产面含 panic 面");
    }
    // 判据-04 判据面零 panic（自扫）。
    let src = include_str!("vcf02_certflow_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("E802-判据-04-判据面零 panic");
    } else {
        set.fail("E802-判据-04-判据面零 panic", "判据面含 panic 面");
    }
}

fn chk_not_truncated(set: &mut CheckSet) {
    // 判据-05 聚合守恒防自调。
    let a = run_vcf02_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == EXPECT_A_COUNT;
    let self_ok = sp + sf == EXPECT_B_BEFORE;
    let no_trunc = !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("E802-判据-05-聚合守恒防自调");
    } else {
        set.fail("E802-判据-05-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

fn chk_carry(set: &mut CheckSet) {
    // 判据-06 版本与摘要承载：摘要行含版本、步数与六步流程原文要素。
    let line = api::screen_line();
    if line.contains(VCF02_VERSION)
        && line.contains("steps=6")
        && line.contains("申请→能力探测→基准执行→判定→认证授予→发布")
    {
        set.ok("E802-判据-06-摘要承载");
    } else {
        set.fail("E802-判据-06-摘要承载", "版本或流程原文漂移");
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 自动化 + 恢复 + 铁律 + 审计。
pub fn run_vcf02_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcf02/a");
    chk_steps(&mut s);
    chk_auto(&mut s);
    chk_resume(&mut s);
    chk_ironlaw(&mut s);
    chk_audit(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vcf02_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcf02/b");
    chk_codes(&mut s);
    chk_chain(&mut s);
    chk_zero_panic(&mut s);
    chk_not_truncated(&mut s);
    chk_carry(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcf02_checks() -> CheckSet {
    CheckSet::merge(run_vcf02_checks_a_standalone(), run_vcf02_checks_b_standalone())
}
