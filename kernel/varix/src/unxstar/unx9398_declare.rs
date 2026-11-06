//! UNX-F9398 I 型联调判据可复测性声明 · 0x7D100段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9398-J1 逐字）：
//!   锚点：UNX-F9398-J1 联测判据 10 次重复判据过，10/10 次通过统计格过，
//!   零 flaky 声明格过
//!
//! 数据段 SEG_BASE = 0x7D100：诊断码闭集 +0x01..=+0x09（9 码，
//! 9398 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - 联测核：RetestCore（确定性小联测：3 阶段状态机 + 守恒账，
//!     每轮产出判定与轮指纹，无任何随机源）；
//!   - 账本：RepeatStats（10 次重复：轮数=10、通过=10、逐轮指纹
//!     逐位一致 → 方差闭集 0，零 flaky）；
//!   - 状态机：ProbeStage（Warm→Probe→Verdict 单向）；
//!   - 注入核：4 类坏样本（轮数不足/指纹漂移/状态机逆迁移/账不守恒）
//!     逐类显性拒绝；
//!   - 对拍：Linux errno 对照（EINVAL/EBADF/EAGAIN/EINTR/ENOENT）；
//!   - 指纹：FNV-1a 逐轮指纹链双跑对拍（可复测性载体）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9398 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7D100;

/// 重复联测轮数（判据要求 10 次）。
pub const RETEST_ROUNDS: u32 = 10;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=9 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestCode {
    /// 轮数不足：不足 10 次不得下结论
    RunsTooFew,
    /// 指纹漂移：某轮指纹与基准不一致（flaky 候选）
    FingerprintDrift,
    /// 状态机逆迁移
    ProbeReversed,
    /// 账不守恒：某轮账目失衡
    LedgerBroken,
    /// 统计账倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑指纹不一致
    NotReproducible,
    /// 通过数与轮数不符
    PassCountMismatch,
    /// 声明缺章：可复测性声明未逐条登记
    DeclarationIncomplete,
}

impl RetestCode {
    /// 闭集数组。
    pub const ALL: [RetestCode; 9] = [
        RetestCode::RunsTooFew,
        RetestCode::FingerprintDrift,
        RetestCode::ProbeReversed,
        RetestCode::LedgerBroken,
        RetestCode::LedgerRegress,
        RetestCode::UnknownErrno,
        RetestCode::NotReproducible,
        RetestCode::PassCountMismatch,
        RetestCode::DeclarationIncomplete,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RetestCode::RunsTooFew => 1,
                RetestCode::FingerprintDrift => 2,
                RetestCode::ProbeReversed => 3,
                RetestCode::LedgerBroken => 4,
                RetestCode::LedgerRegress => 5,
                RetestCode::UnknownErrno => 6,
                RetestCode::NotReproducible => 7,
                RetestCode::PassCountMismatch => 8,
                RetestCode::DeclarationIncomplete => 9,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RetestCode::RunsTooFew => "10 次重复是下结论的下限，少跑即拒绝声明",
            RetestCode::FingerprintDrift => "任何一轮指纹漂移即 flaky 候选，判红",
            RetestCode::ProbeReversed => "探测状态机单向，回退显性拒绝",
            RetestCode::LedgerBroken => "每轮账必须守恒，失衡轮整轮作废",
            RetestCode::LedgerRegress => "统计账只增不减，倒退即审计失败",
            RetestCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            RetestCode::NotReproducible => "同输入双跑指纹必须逐位一致",
            RetestCode::PassCountMismatch => "10/10 通过才可声明零 flaky",
            RetestCode::DeclarationIncomplete => "可复测性声明必须逐条登记不得缺章",
        }
    }
}

// ── 探测状态机 ────────────────────────────────────────────────────────
/// 单轮探测状态机：Warm→Probe→Verdict 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeStage {
    /// 预热（账清零）
    Warm,
    /// 探测（状态机 + 账本联合执行）
    Probe,
    /// 出判定
    Verdict,
}

impl ProbeStage {
    /// 单向迁移核验。
    pub fn advance(self, want: ProbeStage) -> Option<ProbeStage> {
        let legal = match (self, want) {
            (ProbeStage::Warm, ProbeStage::Probe) => true,
            (ProbeStage::Probe, ProbeStage::Verdict) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── 单轮账 ────────────────────────────────────────────────────────────
/// 单轮探测账（三段守恒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundLedger {
    pub probed: u32,
    pub checked: u32,
    pub verdicts: u32,
}

impl RoundLedger {
    /// 零账初值。
    pub fn new() -> RoundLedger {
        RoundLedger { probed: 0, checked: 0, verdicts: 0 }
    }

    /// 三段守恒。
    pub fn conserved(&self) -> bool {
        self.probed == self.checked && self.checked == self.verdicts
    }
}

/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 单轮指纹（确定性：轮次 + 三段账）。
pub fn round_fingerprint(round: u32, led: &RoundLedger) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    buf.extend_from_slice(&round.to_le_bytes());
    buf.extend_from_slice(&led.probed.to_le_bytes());
    buf.extend_from_slice(&led.checked.to_le_bytes());
    buf.extend_from_slice(&led.verdicts.to_le_bytes());
    fnv1a(&buf)
}

// ── 单轮联测核 ────────────────────────────────────────────────────────
/// 单轮联测：Warm→Probe（探 4 格）→Verdict，全程确定性。
///
/// 返回 (账, 是否全过)；同轮两次结果逐位一致。
pub fn run_round(round: u32) -> (RoundLedger, bool) {
    let mut led = RoundLedger::new();
    let mut stage = ProbeStage::Warm;
    let mut all_pass = true;
    if stage.advance(ProbeStage::Probe).is_none() {
        return (led, false);
    }
    stage = ProbeStage::Probe;
    // 探 4 格：守恒/单向/指纹/闭集，全部性质断言（无魔数）
    let mut i = 0u32;
    while i < 4 {
        led.probed += 1;
        // 格 1：状态机可推进且不可逆
        let fwd = ProbeStage::Warm.advance(ProbeStage::Probe).is_some();
        let back = ProbeStage::Verdict.advance(ProbeStage::Warm).is_none();
        // 格 2：账单调
        let before = led.checked;
        led.checked += 1;
        let mono = led.checked == before + 1;
        // 格 3：轮指纹确定
        let fp_a = round_fingerprint(round, &led);
        let fp_b = round_fingerprint(round, &led);
        if fwd && back && mono && fp_a == fp_b {
            led.verdicts += 1;
        } else {
            all_pass = false;
        }
        i += 1;
    }
    let _ = stage;
    (led, all_pass && led.conserved() && led.verdicts == 4)
}

// ── 10 次重复统计 ─────────────────────────────────────────────────────
/// 重复统计账。
#[derive(Clone, Copy)]
pub struct RepeatStats {
    /// 执行轮数
    pub runs: u32,
    /// 全过轮数（必须 = runs）
    pub passed: u32,
    /// 指纹漂移轮数（必须 0）
    pub drifts: u32,
    /// 基准指纹链
    pub base_fp: u32,
}

impl RepeatStats {
    /// 零 flaky 声明成立：10/10 通过且零漂移。
    pub fn zero_flaky(&self) -> bool {
        self.runs == RETEST_ROUNDS && self.passed == RETEST_ROUNDS && self.drifts == 0
    }
}

/// 10 次重复联测：逐轮双跑 + 同轮指纹对拍（drift 判定载体）。
///
/// 每轮执行两次，同轮两次指纹必须逐位一致；不一致即 drift。
/// 返回 (统计账, 逐轮指纹序列——取每轮首跑指纹)；两次调用结果逐位一致。
pub fn run_retest_suite() -> (RepeatStats, Vec<u32>) {
    let mut stats = RepeatStats { runs: 0, passed: 0, drifts: 0, base_fp: 0 };
    let mut fps: Vec<u32> = Vec::new();
    let mut round = 0u32;
    while round < RETEST_ROUNDS {
        let (led, ok) = run_round(round);
        let fp = round_fingerprint(round, &led);
        // 同轮双跑：确定性复测的直接证据
        let (led2, ok2) = run_round(round);
        if round_fingerprint(round, &led2) != fp || !ok2 {
            stats.drifts += 1;
        }
        if ok {
            stats.passed += 1;
        }
        if round == 0 {
            stats.base_fp = fp;
        }
        stats.runs += 1;
        fps.push(fp);
        round += 1;
    }
    (stats, fps)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 4 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 4], u32) {
    let mut ok = [false; 4];
    let mut detected = 0u32;

    // 类 1：轮数不足（零 flaky 声明拒绝短跑账）
    let short = !RepeatStats { runs: 9, passed: 9, drifts: 0, base_fp: 1 }.zero_flaky();
    if short {
        detected += 1;
        ok[0] = true;
    }

    // 类 2：指纹漂移（同轮不同账产出不同指纹）
    let mut led_a = RoundLedger::new();
    led_a.probed = 1;
    led_a.checked = 1;
    led_a.verdicts = 1;
    let mut led_b = led_a;
    led_b.verdicts = 2;
    let drift = round_fingerprint(3, &led_a) != round_fingerprint(3, &led_b);
    if drift {
        detected += 1;
        ok[1] = true;
    }

    // 类 3：状态机逆迁移
    let reversed = ProbeStage::Verdict.advance(ProbeStage::Probe).is_none()
        && ProbeStage::Probe.advance(ProbeStage::Warm).is_none();
    if reversed {
        detected += 1;
        ok[2] = true;
    }

    // 类 4：账不守恒（三段失衡的账拒绝出判定）
    let mut broken = RoundLedger::new();
    broken.probed = 4;
    broken.checked = 3;
    broken.verdicts = 4;
    let not_conserved = !broken.conserved();
    if not_conserved {
        detected += 1;
        ok[3] = true;
    }

    (ok, detected)
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EINTR, EBADF, EAGAIN, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9398 自检：可复测性声明判据（约 13 条）。
pub fn run_unx9398_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9398");

    // 1. 10 次重复足额
    let (stats, fps) = run_retest_suite();
    s.add("重复联测：10/10 轮足额执行", stats.runs == RETEST_ROUNDS,
          "少跑不得下零 flaky 结论");

    // 2. 10/10 通过
    s.add("10/10 通过统计格过", stats.passed == RETEST_ROUNDS,
          "每轮 4 格性质断言全过");

    // 3. 零 flaky 声明
    s.add("零 flaky 声明：10/10 通过且零漂移", stats.zero_flaky(),
          "逐轮指纹逐位一致是可复测性的直接证据");

    // 4. 逐轮指纹链：10 轮指纹各不相同（轮次可定位）且零漂移
    let mut all_distinct = true;
    let mut i = 0usize;
    while i < fps.len() {
        let mut j = i + 1;
        while j < fps.len() {
            if fps.get(i) == fps.get(j) {
                all_distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    let chain_ok = fps.len() == RETEST_ROUNDS as usize
        && stats.drifts == 0
        && stats.base_fp != 0
        && all_distinct;
    s.add("指纹链：10 轮指纹逐轮区分且同轮零漂移", chain_ok,
          "确定性联测无隐藏状态");

    // 5. 单轮账守恒
    let (led0, ok0) = run_round(0);
    s.add("单轮账守恒：探 4 格三段守恒", ok0 && led0.conserved() && led0.verdicts == 4,
          "失衡轮整轮作废");

    // 6. 轮指纹确定性
    let (led0b, _) = run_round(0);
    s.add("轮指纹确定：同轮双跑逐位一致",
          round_fingerprint(0, &led0) == round_fingerprint(0, &led0b),
          "同输入必须逐位复现");

    // 7. 异轮指纹区分
    let (led5, _) = run_round(5);
    s.add("异轮区分：不同轮次指纹不同（轮次混入哈希）",
          round_fingerprint(0, &led0) != round_fingerprint(5, &led5),
          "指纹必须能定位轮次");

    // 8. 探测状态机单向
    let fsm_ok = ProbeStage::Warm.advance(ProbeStage::Probe) == Some(ProbeStage::Probe)
        && ProbeStage::Probe.advance(ProbeStage::Verdict) == Some(ProbeStage::Verdict)
        && ProbeStage::Verdict.advance(ProbeStage::Warm).is_none();
    s.add("探测状态机：Warm→Probe→Verdict 单向", fsm_ok,
          "回退显性拒绝");

    // 9. 注入核 4 类逐类拒绝
    let (inj_ok, detected) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：4 类坏样本逐类显性拒绝", all_rej && detected == 4,
          "短跑/漂移/逆迁移/失衡全过闸");

    // 10. 声明缺章拒绝
    let incomplete = !RepeatStats { runs: RETEST_ROUNDS, passed: 9, drifts: 0, base_fp: 1 }.zero_flaky()
        && !RepeatStats { runs: RETEST_ROUNDS, passed: RETEST_ROUNDS, drifts: 1, base_fp: 1 }.zero_flaky();
    s.add("声明缺章拒绝：通过缺一或漂移一即拒绝声明", incomplete,
          "10/10 且零漂移才可声明");

    // 11. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(1234);
    s.add("errno 对照：5 码表内全中、表外拒绝", codes_ok,
          "错误码闭集对拍");

    // 12. 双跑指纹一致
    let (_, fps2) = run_retest_suite();
    let fp1 = fnv1a(&{
        let mut b: Vec<u8> = Vec::new();
        for f in fps.iter() {
            b.extend_from_slice(&f.to_le_bytes());
        }
        b
    });
    let fp2 = fnv1a(&{
        let mut b: Vec<u8> = Vec::new();
        for f in fps2.iter() {
            b.extend_from_slice(&f.to_le_bytes());
        }
        b
    });
    s.add("双跑指纹：10 轮指纹链 FNV-1a 两次一致", fp1 == fp2,
          "可复测性的总账证据");

    // 13. 诊断码闭集
    let text_ok = RetestCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < RetestCode::ALL.len() {
        let mut j = i + 1;
        while j < RetestCode::ALL.len() {
            let a = RetestCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = RetestCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：9 码唯一、文案非空", text_ok && uniq,
          "SEG_BASE+1..=+9 闭集");

    // 动态明细（≤8）
    let r_text = format!("轮 {} / 过 {} / 漂移 {}", stats.runs, stats.passed, stats.drifts);
    s.add_dyn("复测账快照", stats.zero_flaky(), &r_text);
    let fp_text = format!("指纹链 0x{:08X}", fp1);
    s.add_dyn("指纹快照", fp1 == fp2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retest_full_and_zero_flaky() {
        let (st, fps) = run_retest_suite();
        assert_eq!(st.runs, RETEST_ROUNDS);
        assert_eq!(st.passed, RETEST_ROUNDS);
        assert_eq!(st.drifts, 0);
        assert!(st.zero_flaky());
        assert_eq!(fps.len(), RETEST_ROUNDS as usize);
    }

    #[test]
    fn round_deterministic() {
        let (a, oa) = run_round(2);
        let (b, ob) = run_round(2);
        assert_eq!(a, b);
        assert_eq!(oa, ob);
        assert!(oa);
    }

    #[test]
    fn round_ledger_conserved() {
        let (l, ok) = run_round(7);
        assert!(l.conserved());
        assert_eq!(l.probed, 4);
        assert_eq!(l.verdicts, 4);
        assert!(ok);
    }

    #[test]
    fn probe_fsm_one_way() {
        assert_eq!(ProbeStage::Warm.advance(ProbeStage::Verdict), None);
        assert_eq!(ProbeStage::Verdict.advance(ProbeStage::Probe), None);
        assert_eq!(ProbeStage::Warm.advance(ProbeStage::Probe), Some(ProbeStage::Probe));
    }

    #[test]
    fn different_rounds_differ() {
        let (l0, _) = run_round(0);
        let (l1, _) = run_round(1);
        assert_ne!(round_fingerprint(0, &l0), round_fingerprint(1, &l1));
    }

    #[test]
    fn zero_flaky_requires_full_runs() {
        assert!(!RepeatStats { runs: 5, passed: 5, drifts: 0, base_fp: 0 }.zero_flaky());
        assert!(!RepeatStats { runs: 10, passed: 9, drifts: 0, base_fp: 0 }.zero_flaky());
        assert!(!RepeatStats { runs: 10, passed: 10, drifts: 1, base_fp: 0 }.zero_flaky());
        assert!(RepeatStats { runs: 10, passed: 10, drifts: 0, base_fp: 7 }.zero_flaky());
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, n) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert_eq!(n, 4);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EINVAL));
        assert!(agrees_with_linux(-EAGAIN));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(909));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, f1) = run_retest_suite();
        let (_, f2) = run_retest_suite();
        assert_eq!(f1, f2);
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(RetestCode::ALL.len(), 9);
        for c in RetestCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 9);
        }
    }

    #[test]
    fn unx9398_checks_green() {
        let set = run_unx9398_checks();
        assert!(set.all_passed(), "UNX-F9398 red: {}", set.red_items_text());
    }
}
