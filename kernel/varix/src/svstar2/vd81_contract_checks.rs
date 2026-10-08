//! CGPU-F0481 域自检（CGPU-D 域 · 80 帧合同模型判据层）
//!
//! 判据侧**独立写死**期望（80/72/250 三条款、秩位语义、256 环、五级阶梯、
//! 码段 0x48 四码）。聚合防自调：族内判据只调另一族 standalone + 进行中
//! set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vd81_contract::*;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_CODES: [u16; 4] = [0x4800, 0x4801, 0x4802, 0x4803];
const EXPECT_THEME_COUNT: usize = 8;
const EXPECT_DEGRADE_STEPS: usize = 5;

/// 合格语料：150 帧均匀 12500us（80fps）。
fn good_samples() -> Vec<u32> {
    let mut v = Vec::new();
    let mut i = 0usize;
    while i < 150 {
        v.push(12_500);
        i += 1;
    }
    v
}

/// 违约语料：140 帧合格 + 10 帧 300ms 卡顿。
fn bad_samples() -> Vec<u32> {
    let mut v = good_samples();
    let mut i = 0usize;
    while i < 10 {
        v.push(300_000);
        i += 1;
    }
    v
}

// ---------------------------------------------------------------------------
// 一、规格（八主题 / 可测条款 / 铁律 / 对接）
// ---------------------------------------------------------------------------

fn chk_spec_eight_themes(set: &mut CheckSet) {
    // 规格-01：八主题封闭往返 + 越界 None。
    let mut rt = true;
    let mut i = 0usize;
    while i < EXPECT_THEME_COUNT {
        match FidelityTheme::of_ordinal(i) {
            Some(t) => {
                if t.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    let oor = FidelityTheme::of_ordinal(8).is_none();
    if rt && oor && FidelityTheme::ALL.len() == EXPECT_THEME_COUNT {
        set.ok("ED81-规格-01-八主题封闭往返");
    } else {
        set.fail("ED81-规格-01-八主题封闭往返", "主题漂移或越界未拒");
    }
    // 规格-02：可测条款——合格语料三条全过（可测可判的正面）。
    let v = evaluate(&good_samples());
    let ok = match v {
        Ok(verdict) => {
            verdict.fulfilled
                && verdict.clauses[0].passed
                && verdict.clauses[1].passed
                && verdict.clauses[2].passed
                && verdict.clauses[0].measured == 80
        }
        Err(_) => false,
    };
    if ok {
        set.ok("ED81-规格-02-合格语料三条款全过");
    } else {
        set.fail("ED81-规格-02-合格语料三条款全过", "80fps 均匀语料未兑现合同");
    }
}

fn chk_spec_clause_evidence(set: &mut CheckSet) {
    // 规格-03：违约证据——违约语料逐条转红且卡顿计数恰为 10
    //（判据侧独立写死：10 帧 300ms 卡顿全数计入）。
    let v = evaluate(&bad_samples());
    let ok = match v {
        Ok(verdict) => {
            !verdict.fulfilled
                && verdict.clauses[2].measured == 10
                && !verdict.clauses[2].passed
        }
        Err(_) => false,
    };
    if ok {
        set.ok("ED81-规格-03-违约证据逐条在案");
    } else {
        set.fail("ED81-规格-03-违约证据逐条在案", "卡顿未逐帧入账或条款未翻红");
    }
    // 规格-04：样本不足拒绝（99 < MIN_SAMPLES——噪声结论不如不出）。
    let mut short = good_samples();
    short.truncate(99);
    let r = evaluate(&short);
    if r == Err(E_D481_SAMPLE_SHORT) {
        set.ok("ED81-规格-04-样本不足拒绝");
    } else {
        set.fail("ED81-规格-04-样本不足拒绝", "不足样本未被拒");
    }
    // 规格-05：卡顿阈值恰边界——250_000us 恰计入、249_999 不计
    //（阈值含等判定两侧钉死）。
    let mut at_edge = good_samples();
    at_edge.push(250_000);
    let mut below = good_samples();
    below.push(249_999);
    let at_v = evaluate(&at_edge);
    let below_v = evaluate(&below);
    let ok = match (at_v, below_v) {
        (Ok(a), Ok(b)) => a.clauses[2].measured == 1 && b.clauses[2].measured == 0,
        _ => false,
    };
    if ok {
        set.ok("ED81-规格-05-卡顿阈值恰边界");
    } else {
        set.fail("ED81-规格-05-卡顿阈值恰边界", "阈值含等判定未两侧钉死");
    }
    // 规格-06：分位可测性——掺慢帧后 P99 与 P95 分离（恒等语料放走秩位
    // 漂移是 F0815 教训：语料必须使两分位可分辨）。
    let mut mixed = good_samples();
    let mut i = 0usize;
    while i < 5 {
        mixed.push(20_000); // 50fps 慢帧 5 帧
        i += 1;
    }
    let v = evaluate(&mixed);
    let ok = match v {
        Ok(verdict) => verdict.clauses[0].measured != verdict.clauses[1].measured,
        Err(_) => false,
    };
    if ok {
        set.ok("ED81-规格-06-分位分离可测");
    } else {
        set.fail("ED81-规格-06-分位分离可测", "P95/P99 同值（秩位漂移不可测）");
    }
}

fn chk_spec_timing_feedback(set: &mut CheckSet) {
    // 规格-07：闭环启用是显式动作——未启用 record 拒 0x4801；
    // 启用后 O(1) 入环且均值可算（判据侧独立手算）。
    let mut fb = FrameTimingFeedback::new();
    let off = fb.record(12_500);
    fb.enable();
    let on1 = fb.record(12_500);
    let on2 = fb.record(25_000);
    let avg = fb.avg_frame_us();
    let ok = off == Err(E_D481_TIMING_OFF)
        && on1.is_ok()
        && on2.is_ok()
        && avg == 18_750
        && fb.is_enabled();
    if ok {
        set.ok("ED81-规格-07-闭环启用显式可查");
    } else {
        set.fail("ED81-规格-07-闭环启用显式可查", "未启用未拒或均值漂移");
    }
    // 规格-08：C 域移交对接——契约名落账 + 闭环样本直喂合同判定
    //（反馈闭环端到端）。
    let mut fb = FrameTimingFeedback::new();
    fb.enable();
    let mut i = 0usize;
    while i < 150 {
        let _ = fb.record(12_500);
        i += 1;
    }
    let snap = fb.snapshot();
    let v = evaluate(&snap);
    let ok = C_HANDOFF_CONTRACT == "F0476:frame-timing-feedback"
        && matches!(v, Ok(verdict) if verdict.fulfilled);
    if ok {
        set.ok("ED81-规格-08-C域闭环端到端");
    } else {
        set.fail("ED81-规格-08-C域闭环端到端", "闭环样本未能判定或契约名漂移");
    }
    // 边界-01：环满覆盖最旧——257 条后 snapshot 首条是第 2 条。
    let mut fb = FrameTimingFeedback::new();
    fb.enable();
    let mut i = 0usize;
    while i < 257 {
        let _ = fb.record(i as u32 + 1);
        i += 1;
    }
    let snap = fb.snapshot();
    let ok = snap.len() == 256 && snap[0] == 2 && snap[255] == 257;
    if ok {
        set.ok("ED81-边界-01-环满覆盖最旧");
    } else {
        set.fail("ED81-边界-01-环满覆盖最旧", "环形覆盖语义漂移");
    }
}

fn chk_spec_ironlaw(set: &mut CheckSet) {
    // 规格-09：降质阶梯——逐级降、档名翻面、步数计数（先降质的证据）。
    let mut g = DegradeGuard::new();
    let l1 = g.degrade_step();
    let l2 = g.degrade_step();
    let ok = l1 == Ok(1)
        && l2 == Ok(2)
        && g.degrade_steps == 2
        && g.current_label() == "后处理"
        && g.ironlaw_intact();
    if ok {
        set.ok("ED81-规格-09-降质阶梯");
    } else {
        set.fail("ED81-规格-09-降质阶梯", "阶梯/档名/步数漂移");
    }
    // 规格-10：铁律重申——阶梯到底（5 级全降）再降 = 降帧事故
    //（0x4802 + 事故计数 + ironlaw 翻假）。
    let mut g = DegradeGuard::new();
    let mut i = 0usize;
    let mut last = Ok(0usize);
    while i < EXPECT_DEGRADE_STEPS {
        last = g.degrade_step();
        i += 1;
    }
    let exhausted = last.is_ok() && g.level == EXPECT_DEGRADE_STEPS;
    let drop = g.degrade_step();
    let ok = exhausted
        && drop == Err(E_D481_FRAME_DROP)
        && g.frame_drop_incidents == 1
        && !g.ironlaw_intact();
    if ok {
        set.ok("ED81-规格-10-降帧即事故铁律");
    } else {
        set.fail("ED81-规格-10-降帧即事故铁律", "阶梯到底未记事故或铁律未翻面");
    }
}

fn chk_idem_determinism(set: &mut CheckSet) {
    // 幂等-01：判定确定性——同样本两次判定同 verdict。
    let s = bad_samples();
    let v1 = evaluate(&s);
    let v2 = evaluate(&s);
    if v1.is_ok() && v1 == v2 {
        set.ok("ED81-幂等-01-判定确定性");
    } else {
        set.fail("ED81-幂等-01-判定确定性", "同样本不同判决");
    }
    // 幂等-02：判定不改变样本语料（只读入参语义）。
    let s = good_samples();
    let first = s[0];
    let _ = evaluate(&s);
    let untouched = s[0] == first && s.len() == 150;
    if untouched {
        set.ok("ED81-幂等-02-判定只读语料");
    } else {
        set.fail("ED81-幂等-02-判定只读语料", "判定污染了入参样本");
    }
}

// ---------------------------------------------------------------------------
// 三、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

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

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vd81_contract_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("ED81-判据-01-判据面零 panic");
    } else {
        set.fail("ED81-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-11：生产面零 panic（扫实现文件）。
    let src = include_str!("vd81_contract.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("ED81-规格-11-生产面零 panic");
    } else {
        set.fail("ED81-规格-11-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——四码皆 0x48 细分段、互异、与写死值逐位等。
    let got = [E_D481_CLAUSE_FAIL, E_D481_TIMING_OFF, E_D481_FRAME_DROP, E_D481_SAMPLE_SHORT];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x4800 || got[i] != EXPECT_CODES[i] {
            ok = false;
        }
        let mut j = i + 1;
        while j < got.len() {
            if got[i] == got[j] {
                ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("ED81-判据-02-码段独占独立复核");
    } else {
        set.fail("ED81-判据-02-码段独占独立复核", "码漂移或撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 13 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vd81_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 13;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("ED81-判据-03-聚合守恒防自调");
    } else {
        set.fail("ED81-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格+边界+幂等 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格 + 边界 + 幂等。
pub fn run_vd81_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vd81/a");
    chk_spec_eight_themes(&mut s);
    chk_spec_clause_evidence(&mut s);
    chk_spec_timing_feedback(&mut s);
    chk_spec_ironlaw(&mut s);
    chk_idem_determinism(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vd81_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vd81/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vd81_checks() -> CheckSet {
    CheckSet::merge(run_vd81_checks_a_standalone(), run_vd81_checks_b_standalone())
}
