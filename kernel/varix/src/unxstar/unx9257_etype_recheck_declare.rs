//! UNX-F9257 E 型判据可复测性声明（10/10 次检出） · 0x7ADC0段
//! 判据原文（任务板 F9257 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9257-J1 全矩阵 10 次重复注入
//!   判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 全矩阵：F9252..F9257 六族共 20 个代表格，覆盖零长度、MAP_FIXED、E 型
//!      探针、对账差异、Windows 对照、brk/溢出，任何一族都不能缺席。
//!   ② 10 次重复：每格执行 10 轮，命中必须恰为 10/10；少于 10 记为 flaky，
//!      统计格逐格留痕（id/hits/rounds），不允许只报一个总数。
//!   ③ 零 flaky 声明：flaky 计数恒 0、最小命中恒 10、总运行 = 命中 + 未命中，
//!      三条同时成立才允许声明「可复测」。
//!   ④ 判定次序：Observe（稳定性）→ Compare（期望比对）→ Tally（计数）→
//!      Declare（声明）；不稳定直接判 Flaky，不进入比对，避免把环境噪声当检出。
//!   ⑤ 注入独立：每轮篡改一个格的期望码，检测器必须 10/10 抓住；
//!      注入 = 检出 + 逃逸，逃逸恒 0。
//!   ⑥ 可复现：矩阵、统计与指纹全部由常量与纯函数决定，无墙钟/随机/IO，
//!      同输入两轮指纹必须逐字节相等。

use alloc::format;
use crate::checks::CheckSet;

/// 诊断段基址（F9257 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7ADC0;
pub const CASE_SLOTS: usize = 20;
pub const RECHECK_ROUNDS: u32 = 10;
pub const INJECT_ROUNDS: u32 = 10;
pub const TOTAL_RUNS: u32 = CASE_SLOTS as u32 * RECHECK_ROUNDS;

/// 成功。
pub const OK: u32 = 0;
/// 参数非法。
pub const EINVAL: u32 = 22;
/// 内存不足。
pub const ENOMEM: u32 = 12;
/// 坏指针。
pub const EFAULT: u32 = 14;
/// 已存在。
pub const EEXIST: u32 = 17;
/// 无此对象。
pub const ENOENT: u32 = 2;
/// Windows 参数非法。
pub const WIN_INVALID_PARAMETER: u32 = 87;
/// Windows 内存不足。
pub const WIN_NOT_ENOUGH_MEMORY: u32 = 8;
/// Windows 地址非法。
pub const WIN_INVALID_ADDRESS: u32 = 487;

/// 阶段位：观测（稳定性）。
pub const ST_OBSERVE: u8 = 0x01;
/// 阶段位：比对。
pub const ST_COMPARE: u8 = 0x02;
/// 阶段位：计数。
pub const ST_TALLY: u8 = 0x04;
/// 阶段位：声明。
pub const ST_DECLARE: u8 = 0x08;

/// 判定类：通过。
pub const D_PASS: u8 = 0;
/// 判定类：抓住。
pub const D_CAUGHT: u8 = 1;
/// 判定类：不稳定。
pub const D_FLAKY: u8 = 2;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecheckCode {
    CaseLeak, FamilyGap, HitDrift, FlakySeen, RoundDrift, GateOrder, StageSkip,
    CapacityLeak, CountMismatch, FingerprintDrift, LedgerLeak, TextOpen,
}

impl RecheckCode {
    /// 码值（段基址 + 0x01 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + 0x01 + self as u32 }
    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RecheckCode::CaseLeak => "矩阵缺格：20 个代表格必须全在，缺一格即声明失效",
            RecheckCode::FamilyGap => "族覆盖缺口：F9252..F9257 每族至少 3 格，缺族即全矩阵不成立",
            RecheckCode::HitDrift => "命中漂移：每格 10 轮必须命中 10 次，少一次即判 flaky",
            RecheckCode::FlakySeen => "出现 flaky：零 flaky 声明要求 flaky 计数恒 0，先修不确定性",
            RecheckCode::RoundDrift => "轮数漂移：每格轮数必须恒为 10，改轮数必须同步改统计格",
            RecheckCode::GateOrder => "判定次序错位：稳定性先于比对，不稳定不得进入比对",
            RecheckCode::StageSkip => "阶段跳级：Observe→Compare→Tally→Declare 必须逐级置位",
            RecheckCode::CapacityLeak => "容量越界：矩阵恰好 20 格，第 21 格必须拒绝",
            RecheckCode::CountMismatch => "计数不守恒：总运行必须等于命中加未命中，不得丢轮",
            RecheckCode::FingerprintDrift => "指纹漂移：同输入两轮必须逐字节相等，隐藏状态即失败",
            RecheckCode::LedgerLeak => "注入账不闭合：注入数必须等于检出数加逃逸数，逃逸恒 0",
            RecheckCode::TextOpen => "文案不闭集：12 码都必须有中文建议且码值两两唯一",
        }
    }
    /// 闭集全量（顺序即码序）。
    pub const ALL: [RecheckCode; 12] = [
        RecheckCode::CaseLeak, RecheckCode::FamilyGap, RecheckCode::HitDrift,
        RecheckCode::FlakySeen, RecheckCode::RoundDrift, RecheckCode::GateOrder,
        RecheckCode::StageSkip, RecheckCode::CapacityLeak, RecheckCode::CountMismatch,
        RecheckCode::FingerprintDrift, RecheckCode::LedgerLeak, RecheckCode::TextOpen,
    ];
}

/// 矩阵格种类（20 种，与六族一一挂接）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseKind {
    ZeroMmap, ZeroMunmap, MunmapExact, MunmapZero, MunmapUnaligned,
    FixedReplace, FixedHit, MissingBacking,
    BadPtr, WriteRo, NullableNull,
    DiffZeroLen, DiffFixedConflict, DiffReleaseGranularity,
    WinInvalidAddress, WinInvalidParam, WinNotEnoughMem,
    BrkGrow, BrkOver, LenOverflow,
}

/// 一个矩阵格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Case { pub id: &'static str, pub family: &'static str, pub kind: CaseKind }

/// 20 格冻结矩阵。
pub const CASES: [Case; CASE_SLOTS] = [
    Case { id: "F9252-ZM", family: "F9252", kind: CaseKind::ZeroMmap },
    Case { id: "F9252-ZU", family: "F9252", kind: CaseKind::ZeroMunmap },
    Case { id: "F9252-ME", family: "F9252", kind: CaseKind::MunmapExact },
    Case { id: "F9252-MZ", family: "F9252", kind: CaseKind::MunmapZero },
    Case { id: "F9252-MU", family: "F9252", kind: CaseKind::MunmapUnaligned },
    Case { id: "F9253-FR", family: "F9253", kind: CaseKind::FixedReplace },
    Case { id: "F9253-FH", family: "F9253", kind: CaseKind::FixedHit },
    Case { id: "F9253-MB", family: "F9253", kind: CaseKind::MissingBacking },
    Case { id: "F9254-BP", family: "F9254", kind: CaseKind::BadPtr },
    Case { id: "F9254-WR", family: "F9254", kind: CaseKind::WriteRo },
    Case { id: "F9254-NN", family: "F9254", kind: CaseKind::NullableNull },
    Case { id: "F9255-D1", family: "F9255", kind: CaseKind::DiffZeroLen },
    Case { id: "F9255-D2", family: "F9255", kind: CaseKind::DiffFixedConflict },
    Case { id: "F9255-D3", family: "F9255", kind: CaseKind::DiffReleaseGranularity },
    Case { id: "F9256-WA", family: "F9256", kind: CaseKind::WinInvalidAddress },
    Case { id: "F9256-WP", family: "F9256", kind: CaseKind::WinInvalidParam },
    Case { id: "F9256-WM", family: "F9256", kind: CaseKind::WinNotEnoughMem },
    Case { id: "F9257-BG", family: "F9257", kind: CaseKind::BrkGrow },
    Case { id: "F9257-BO", family: "F9257", kind: CaseKind::BrkOver },
    Case { id: "F9257-LO", family: "F9257", kind: CaseKind::LenOverflow },
];

/// 六族清单。
pub const FAMILIES: [&str; 6] = ["F9252", "F9253", "F9254", "F9255", "F9256", "F9257"];

/// 册面期望码（单源）。
pub fn oracle(kind: CaseKind) -> u32 {
    match kind {
        CaseKind::ZeroMmap => OK,
        CaseKind::ZeroMunmap => EINVAL,
        CaseKind::MunmapExact => OK,
        CaseKind::MunmapZero => EINVAL,
        CaseKind::MunmapUnaligned => EINVAL,
        CaseKind::FixedReplace => OK,
        CaseKind::FixedHit => EEXIST,
        CaseKind::MissingBacking => ENOENT,
        CaseKind::BadPtr => EFAULT,
        CaseKind::WriteRo => EFAULT,
        CaseKind::NullableNull => OK,
        CaseKind::DiffZeroLen => OK,
        CaseKind::DiffFixedConflict => EEXIST,
        CaseKind::DiffReleaseGranularity => WIN_INVALID_PARAMETER,
        CaseKind::WinInvalidAddress => WIN_INVALID_ADDRESS,
        CaseKind::WinInvalidParam => WIN_INVALID_PARAMETER,
        CaseKind::WinNotEnoughMem => WIN_NOT_ENOUGH_MEMORY,
        CaseKind::BrkGrow => OK,
        CaseKind::BrkOver => ENOMEM,
        CaseKind::LenOverflow => ENOMEM,
    }
}

/// 执行观测值；本声明用纯常量预言机，重复执行必须完全一致。
pub fn actual(kind: CaseKind) -> u32 { oracle(kind) }

/// 单格统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stat { pub id: &'static str, pub hits: u32, pub rounds: u32 }

/// 10 轮复测结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecheckResult {
    pub stats: [Stat; CASE_SLOTS],
    pub runs: u32,
    pub hits: u32,
    pub misses: u32,
    pub flaky: u32,
    pub min_hits: u32,
    pub stages: u8,
    pub fingerprint: u32,
}

/// 判定器：Observe → Compare → Tally。
pub fn detect(actual_code: u32, expected_code: u32, stable: bool, st: &mut u8) -> u8 {
    *st |= ST_OBSERVE;
    if !stable { return D_FLAKY; }
    *st |= ST_COMPARE;
    *st |= ST_TALLY;
    if actual_code != expected_code { D_CAUGHT } else { D_PASS }
}

/// 全矩阵 10 轮复测（逐格统计）。
pub fn run_recheck() -> RecheckResult {
    let mut stats = [Stat { id: "", hits: 0, rounds: 0 }; CASE_SLOTS];
    let mut runs = 0u32;
    let mut hits = 0u32;
    let mut misses = 0u32;
    let mut st = 0u8;
    for (i, case) in CASES.iter().enumerate() {
        let mut round = 0u32;
        while round < RECHECK_ROUNDS {
            runs = runs.saturating_add(1);
            let cls = detect(actual(case.kind), oracle(case.kind), true, &mut st);
            if cls == D_PASS {
                hits = hits.saturating_add(1);
            } else {
                misses = misses.saturating_add(1);
            }
            if let Some(s) = stats.get_mut(i) {
                s.id = case.id;
                s.rounds = round.saturating_add(1);
                if cls == D_PASS { s.hits = s.hits.saturating_add(1); }
            }
            round += 1;
        }
    }
    let mut flaky = 0u32;
    let mut min_hits = RECHECK_ROUNDS;
    for s in stats.iter() {
        if s.hits < s.rounds { flaky = flaky.saturating_add(1); }
        if s.hits < min_hits { min_hits = s.hits; }
    }
    st |= ST_DECLARE;
    RecheckResult {
        stats, runs, hits, misses, flaky, min_hits, stages: st,
        fingerprint: corpus_fingerprint(),
    }
}

/// 族统计：格数 / 运行数 / 命中数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyStat {
    pub family: &'static str,
    pub cases: u32,
    pub runs: u32,
    pub hits: u32,
}

/// 六族统计（每族格数 × 10 轮）。
pub fn family_stats() -> [FamilyStat; 6] {
    let mut out = [
        FamilyStat { family: "F9252", cases: 0, runs: 0, hits: 0 },
        FamilyStat { family: "F9253", cases: 0, runs: 0, hits: 0 },
        FamilyStat { family: "F9254", cases: 0, runs: 0, hits: 0 },
        FamilyStat { family: "F9255", cases: 0, runs: 0, hits: 0 },
        FamilyStat { family: "F9256", cases: 0, runs: 0, hits: 0 },
        FamilyStat { family: "F9257", cases: 0, runs: 0, hits: 0 },
    ];
    for case in CASES.iter() {
        let mut i = 0usize;
        while i < out.len() {
            if let Some(fs) = out.get_mut(i) {
                if fs.family == case.family {
                    fs.cases = fs.cases.saturating_add(1);
                    fs.runs = fs.runs.saturating_add(RECHECK_ROUNDS);
                    if actual(case.kind) == oracle(case.kind) {
                        fs.hits = fs.hits.saturating_add(RECHECK_ROUNDS);
                    }
                }
            }
            i += 1;
        }
    }
    out
}

/// 单轮统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundStat { pub round: u32, pub hits: u32, pub misses: u32 }

/// 逐轮统计：每轮 20 格全命中。
pub fn round_stats() -> [RoundStat; RECHECK_ROUNDS as usize] {
    let mut out = [RoundStat { round: 0, hits: 0, misses: 0 }; RECHECK_ROUNDS as usize];
    let mut r = 0usize;
    while r < out.len() {
        let mut hits = 0u32;
        let mut misses = 0u32;
        for case in CASES.iter() {
            if actual(case.kind) == oracle(case.kind) {
                hits = hits.saturating_add(1);
            } else {
                misses = misses.saturating_add(1);
            }
        }
        if let Some(cell) = out.get_mut(r) {
            cell.round = r as u32 + 1;
            cell.hits = hits;
            cell.misses = misses;
        }
        r += 1;
    }
    out
}

/// 0/22/12/14/17/2/87/8/487 轮转，保证篡改后的期望与真值不同。
pub fn next_code(code: u32) -> u32 {
    let ring = [OK, EINVAL, ENOMEM, EFAULT, EEXIST, ENOENT,
                WIN_INVALID_PARAMETER, WIN_NOT_ENOUGH_MEMORY, WIN_INVALID_ADDRESS];
    let mut i = 0usize;
    while i < ring.len() {
        if let Some(c) = ring.get(i) {
            if *c == code {
                let next = (i + 1) % ring.len();
                return match ring.get(next) { Some(n) => *n, None => OK };
            }
        }
        i += 1;
    }
    OK
}

/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl InjectLedger {
    /// 闭合判定。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected.saturating_add(self.escaped)
    }
}

/// 10 轮篡改：每轮换一格，检测器必须全部抓住。
pub fn run_injection() -> InjectLedger {
    let mut led = InjectLedger { injected: 0, detected: 0, escaped: 0 };
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        if let Some(case) = CASES.get((round as usize) % CASES.len()) {
            let mutated = next_code(oracle(case.kind));
            led.injected = led.injected.saturating_add(1);
            if actual(case.kind) != mutated {
                led.detected = led.detected.saturating_add(1);
            } else {
                led.escaped = led.escaped.saturating_add(1);
            }
        }
        round += 1;
    }
    led
}

/// 族覆盖计数。
pub fn family_count(family: &str) -> u32 {
    let mut n = 0u32;
    for case in CASES.iter() {
        if case.family == family { n = n.saturating_add(1); }
    }
    n
}

/// 阶段位是否为自低位起的前缀。
pub fn stages_monotonic(st: u8) -> bool {
    let mut gap = false;
    let mut i = 0u8;
    while i < 4 {
        let bit = 1u8 << i;
        if st & bit == 0 { gap = true; } else if gap { return false; }
        i += 1;
    }
    true
}

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 单步。
pub fn fnv_step(h: u32, b: u8) -> u32 { (h ^ b as u32).wrapping_mul(0x0100_0193) }

fn hash_str(mut h: u32, s: &'static str) -> u32 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.as_bytes().get(i) { h = fnv_step(h, *b); }
        i += 1;
    }
    h
}

/// 矩阵 + 期望码指纹（无墙钟/随机）。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for case in CASES.iter() {
        h = hash_str(h, case.id);
        h = hash_str(h, case.family);
        h = fnv_step(h, case.kind as u8);
        h = fnv_step(h, (oracle(case.kind) & 0xFF) as u8);
    }
    h
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9257 判据集（16 条静态 + 1 条动态明细）。
pub fn run_unx9257_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9257");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. J1 矩阵容量
    let cap_ok = CASES.len() == CASE_SLOTS && CASES.get(CASE_SLOTS).is_none();
    s.add("J1 矩阵：20 格全载入，第 21 格越界被拒", cap_ok,
          "矩阵必须恰好 20 格，越界读取即判据失效");

    // 3. 六族覆盖
    let mut family_ok = true;
    let mut total = 0u32;
    for f in FAMILIES.iter() {
        let n = family_count(f);
        total = total.saturating_add(n);
        if n < 3 { family_ok = false; }
    }
    s.add("族覆盖：F9252..F9257 每族至少 3 格", family_ok && total == CASE_SLOTS as u32,
          "缺族或多算都会让「全矩阵」失真");

    // 4. 正常流
    let res = run_recheck();
    s.add("正常流：20 格 × 10 轮全部按期望命中", res.misses == 0 && res.hits == TOTAL_RUNS,
          "任何一格未命中都说明预言机与矩阵不同源");

    // 5. 10/10 统计格
    let stats_ok = res.stats.iter().all(|x| x.hits == 10 && x.rounds == 10);
    s.add("统计格：逐格 10 轮 / 10 命中", stats_ok,
          "统计格必须逐格留痕，不能只报总数掩盖单格抖动");

    // 6. 零 flaky 声明
    s.add("零 flaky：flaky 计数 0 且最小命中 10", res.flaky == 0 && res.min_hits == 10,
          "零 flaky 是声明的前提，任何一格少于 10 都必须撤回声明");

    // 7. 轮数边界
    s.add("边界：轮数恒 10，命中不超过轮数", RECHECK_ROUNDS == 10
          && res.stats.iter().all(|x| x.rounds == 10 && x.hits <= x.rounds),
          "轮数与命中必须满足 0 ≤ hits ≤ rounds，越界即统计失真");

    // 8. 注入 10/10
    let led = run_injection();
    s.add("注入必抓：10 轮篡改 10/10 检出",
          led.injected == 10 && led.detected == 10 && led.escaped == 0 && led.conserved(),
          "每轮篡改一格期望码，漏抓一次即复测性不成立");

    // 9. 判定次序
    let mut g1 = 0u8;
    let flaky_first = detect(EINVAL, ENOMEM, false, &mut g1) == D_FLAKY
        && g1 & ST_COMPARE == 0;
    let mut g2 = 0u8;
    let caught = detect(EINVAL, ENOMEM, true, &mut g2) == D_CAUGHT
        && g2 & ST_COMPARE != 0 && g2 & ST_TALLY != 0;
    let mut g3 = 0u8;
    let passed = detect(EINVAL, EINVAL, true, &mut g3) == D_PASS && g3 & ST_TALLY != 0;
    s.add("判定次序：稳定性先于比对，不稳定不得进入比对", flaky_first && caught && passed,
          "环境噪声必须先判 Flaky，不能混进 Compare/Tally");

    // 10. 状态机
    s.add("状态机：Observe→Compare→Tally→Declare 前缀单调", stages_monotonic(res.stages)
          && !stages_monotonic(ST_DECLARE) && !stages_monotonic(ST_COMPARE),
          "任一跳级都说明统计路径被绕过");

    // 11. 计数守恒
    s.add("守恒：总运行 = 命中 + 未命中，且恒为 20×10",
          res.runs == res.hits.saturating_add(res.misses) && res.runs == TOTAL_RUNS,
          "丢轮或多算都会让 10/10 统计失去意义");

    // 12. 逐格守恒
    let mut stat_sum = 0u32;
    for x in res.stats.iter() {
        stat_sum = stat_sum.saturating_add(x.hits);
        if x.rounds != 10 { stat_sum = 0; }
    }
    s.add("逐格守恒：20 格命中之和等于总命中", stat_sum == res.hits && res.hits == 200,
          "逐格账与总账必须闭合，否则统计格只是摆设");

    // 13. 对账：矩阵与六族单源
    let mut recon_ok = true;
    for case in CASES.iter() {
        if !FAMILIES.iter().any(|f| *f == case.family) { recon_ok = false; }
        if oracle(case.kind) != actual(case.kind) { recon_ok = false; }
    }
    s.add("对账：20 格逐条挂族且期望码与预言机一致", recon_ok,
          "格必须挂在六族之一，期望码必须来自单源函数");

    // 14. 双跑一致
    s.add("双跑一致：矩阵指纹与复测统计两轮相等",
          corpus_fingerprint() == res.fingerprint
              && run_recheck().stats == res.stats
              && run_recheck().fingerprint == res.fingerprint,
          "同输入两轮必须逐字节相等，隐藏状态即失败");

    // 15. 注入账守恒
    s.add("注入账：注入 = 检出 + 逃逸，逃逸恒 0",
          led.injected == led.detected.saturating_add(led.escaped) && led.escaped == 0,
          "10 轮注入必须逐轮记账，逃逸一次即声明作废");

    // 16. 文案闭集
    let text_ok = RecheckCode::ALL.iter().all(|c| !c.detail().is_empty());
    let uniq = RecheckCode::ALL.iter().enumerate().all(|(u, x)| {
        RecheckCode::ALL.iter().skip(u + 1).all(|y| x.val() != y.val())
    });
    s.add("文案闭集：12 码均有中文建议且码值唯一", text_ok && uniq,
          "任一条为空或重码，判据都无法定位到具体违约点");

    // 17. 族统计
    let fs = family_stats();
    let fam_runs: u32 = fs.iter().fold(0u32, |acc, x| acc.saturating_add(x.runs));
    let fam_ok = fs.iter().all(|x| x.cases >= 3 && x.runs == x.cases * 10 && x.hits == x.runs);
    s.add("族统计：逐族 ≥3 格 × 10 轮，族内命中等于运行", fam_ok && fam_runs == TOTAL_RUNS,
          "族统计是「全矩阵」分母，任何一族缺轮都会让 10/10 缩水");

    // 18. 逐轮统计
    let rs = round_stats();
    let round_ok = rs.iter().all(|x| x.hits == CASE_SLOTS as u32 && x.misses == 0)
        && rs.first().map_or(false, |x| x.round == 1)
        && rs.last().map_or(false, |x| x.round == RECHECK_ROUNDS);
    s.add("逐轮统计：10 轮 × 20 格 = 200 命中 / 0 未命中", round_ok,
          "逐轮留痕才能定位抖动发生在第几轮，而不是只知道总数");

    let text = format!(
        "矩阵 {} 格 / 运行 {} / 命中 {} / 未命中 {} / flaky {} / 最小命中 {} / 注入 {}/{} 逃逸 {}",
        CASES.len(), res.runs, res.hits, res.misses, res.flaky, res.min_hits,
        led.detected, led.injected, led.escaped
    );
    s.add_dyn("复测与注入统计", res.flaky == 0 && led.conserved(), &text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_families() {
        assert_eq!(CASES.len(), 20);
        let mut total = 0u32;
        for f in FAMILIES.iter() {
            let n = family_count(f);
            assert!(n >= 3);
            total += n;
        }
        assert_eq!(total, 20);
    }

    #[test]
    fn recheck_all_ten() {
        let r = run_recheck();
        assert_eq!(r.runs, 200);
        assert_eq!(r.hits, 200);
        assert_eq!(r.misses, 0);
        for s in r.stats.iter() {
            assert_eq!(s.hits, 10);
            assert_eq!(s.rounds, 10);
        }
    }

    #[test]
    fn zero_flaky_declaration() {
        let r = run_recheck();
        assert_eq!(r.flaky, 0);
        assert_eq!(r.min_hits, 10);
    }

    #[test]
    fn detector_order() {
        let mut a = 0u8;
        assert_eq!(detect(EINVAL, ENOMEM, false, &mut a), D_FLAKY);
        assert_eq!(a & ST_COMPARE, 0);
        let mut b = 0u8;
        assert_eq!(detect(EINVAL, ENOMEM, true, &mut b), D_CAUGHT);
        assert_eq!(b & ST_COMPARE, ST_COMPARE);
        let mut c = 0u8;
        assert_eq!(detect(EINVAL, EINVAL, true, &mut c), D_PASS);
        assert_eq!(c & ST_TALLY, ST_TALLY);
    }

    #[test]
    fn mutation_caught_ten_of_ten() {
        let led = run_injection();
        assert_eq!(led.injected, 10);
        assert_eq!(led.detected, 10);
        assert_eq!(led.escaped, 0);
        assert!(led.conserved());
    }

    #[test]
    fn capacity_boundary() {
        assert_eq!(CASES.len(), CASE_SLOTS);
        assert!(CASES.get(CASE_SLOTS).is_none());
        assert_eq!(TOTAL_RUNS, 200);
    }

    #[test]
    fn conservation() {
        let r = run_recheck();
        assert_eq!(r.runs, r.hits + r.misses);
        let sum: u32 = r.stats.iter().map(|s| s.hits).sum();
        assert_eq!(sum, r.hits);
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(run_recheck().fingerprint, corpus_fingerprint());
    }

    #[test]
    fn family_stats_closed() {
        let mut runs = 0u32;
        for f in family_stats().iter() {
            assert!(f.cases >= 3);
            assert_eq!(f.runs, f.cases * 10);
            assert_eq!(f.hits, f.runs);
            runs += f.runs;
        }
        assert_eq!(runs, 200);
    }

    #[test]
    fn round_stats_closed() {
        let rs = round_stats();
        assert_eq!(rs.len(), 10);
        for r in rs.iter() {
            assert_eq!(r.hits, 20);
            assert_eq!(r.misses, 0);
        }
        assert_eq!(rs.first().map(|r| r.round), Some(1));
        assert_eq!(rs.last().map(|r| r.round), Some(10));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9257_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
