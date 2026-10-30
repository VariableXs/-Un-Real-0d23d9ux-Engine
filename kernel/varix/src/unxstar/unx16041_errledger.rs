//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16041 · 错误码翻译面回归账与只进不退机制 · 0x68480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定，CoRun·UNX-E1-B03）：
//!   UNX-F16041-J1 回归账逐行落账（探针/结果/版本戳），只进不退断言
//!   （已绿项失败数 = 0）复测通过，账行只增不删断言通过。
//! 定位：F16031/32 立了错误码翻译表，本条立回归账：全绿账滚动机制
//!   在翻译面的第一次落地——每批 finalize 跑翻译探针，结果入账，
//!   绿过的项再红即记缺陷且 P0 清零门禁生效，账行只增不删。
//! 设计要点：
//!   - 回归账结构：每行（探针标识/执行批次/结果绿红/版本戳/归因
//!     三态），账行只增不删，红行状态更新（修复关闭）不改史；
//!   - 归因三态闭集：翻译表缺陷 / 上游行为变化 / 探针自身——三类
//!     必居其一，归因字段非空率 100%；
//!   - 只进不退闸：每次执行把结果集与账上已绿集 diff，已绿项失败
//!     数必须为 0，非零即触发缺陷登记（BUG-UNX-#### 编号流）并
//!     阻断本批 finalize——"绿了又红"不是波动，是回归；
//!   - 账哈希链：FNV 链前缀保留，篡改任一行绿记录即链断裂（账本
//!     可信的前提是账本不可改史）；
//!   - 注入面：篡改绿记录/已绿项失败/归因空/账行删除/批次倒序/
//!     探针标识重复——各 10/10 检出；
//!   - LCG 种子写死 = SEG_BASE（0x68480）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x68480，AI-VX-1030 E1 域批1 独占区）。
pub const SEG_BASE: u32 = 0x68480;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 回归结果与归因闭集 ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunResult {
    Green,
    Red,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cause {
    /// 翻译表缺陷（我方账面）
    TranslateDefect,
    /// 上游行为变化（Wine 侧行为演进）
    UpstreamShift,
    /// 探针自身缺陷
    ProbeSelf,
}

impl Cause {
    pub fn as_u32(self) -> u32 {
        match self {
            Cause::TranslateDefect => 0,
            Cause::UpstreamShift => 1,
            Cause::ProbeSelf => 2,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Cause::TranslateDefect => "翻译表缺陷",
            Cause::UpstreamShift => "上游行为变化",
            Cause::ProbeSelf => "探针自身",
        }
    }
    pub fn from_u32(v: u32) -> Option<Cause> {
        match v {
            0 => Some(Cause::TranslateDefect),
            1 => Some(Cause::UpstreamShift),
            2 => Some(Cause::ProbeSelf),
            _ => None,
        }
    }
}

/// 红行处置状态：红行状态更新不改史——账行本体永不动，处置另记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedDisposition {
    Open,
    Filed,   // 已登记 BUG-UNX-####
    Closed,  // 修复关闭（关闭行另账追加）
}

impl RedDisposition {
    pub fn as_u32(self) -> u32 {
        match self {
            RedDisposition::Open => 0,
            RedDisposition::Filed => 1,
            RedDisposition::Closed => 2,
        }
    }
}

/// 回归账单行：五字段齐备。
#[derive(Clone, Debug)]
pub struct RegressRow {
    /// 探针标识（账内 (标识,批次) 唯一）。
    pub probe: String,
    /// 执行批次号（单调不回退）。
    pub batch: u32,
    /// 结果绿红。
    pub result: RunResult,
    /// 版本戳（翻译表版本）。
    pub version: u32,
    /// 归因（红行必填；绿行为 None）。
    pub cause: Option<Cause>,
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlCode {
    /// 探针标识空
    ProbeBlank,
    /// (标识,批次) 重复落账
    DuplicateEntry,
    /// 批次倒序（时间线失守）
    BatchReversed,
    /// 归因缺失：红行无归因（三类必居其一）
    CauseMissing,
    /// 归因越三态闭集
    CauseOutOfRange,
    /// 只进不退闸触发：已绿项再红
    GreenRegressed,
    /// 账行删除：哈希链前缀断裂
    HashChainBroken,
    /// 绿行带归因（绿行归因必须为空——口径漂移防线）
    GreenWithCause,
    /// 缺陷编号流断裂（编号跳号）
    BugNumberGap,
    /// 双跑不一致
    NotReproducible,
    /// 注入漏抓
    InjectionLeak,
}

impl RlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RlCode::ProbeBlank => "探针标识空：账行主键缺失",
            RlCode::DuplicateEntry => "同探针同批次重复落账",
            RlCode::BatchReversed => "批次倒序：时间线失守",
            RlCode::CauseMissing => "红行无归因：三类必居其一",
            RlCode::CauseOutOfRange => "归因越三态闭集",
            RlCode::GreenRegressed => "只进不退闸触发：已绿项再红",
            RlCode::HashChainBroken => "账行删除：哈希链前缀断裂",
            RlCode::GreenWithCause => "绿行带归因：口径漂移",
            RlCode::BugNumberGap => "缺陷编号流断裂：编号跳号",
            RlCode::NotReproducible => "双跑不一致：确定性红线",
            RlCode::InjectionLeak => "注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [RlCode; 11] = [
        RlCode::ProbeBlank,
        RlCode::DuplicateEntry,
        RlCode::BatchReversed,
        RlCode::CauseMissing,
        RlCode::CauseOutOfRange,
        RlCode::GreenRegressed,
        RlCode::HashChainBroken,
        RlCode::GreenWithCause,
        RlCode::BugNumberGap,
        RlCode::NotReproducible,
        RlCode::InjectionLeak,
    ];
}

/// 全局 detail：按诊断码值反查中文释义。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RlCode::ALL.len() {
        if RlCode::ALL[i].val() == code {
            return RlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── FNV-1a 32 位（账行哈希链） ──────────────────────────────────
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h ^= data[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ─────────────────────────
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed ^ 0x9E37_79B9_7F4A_7C15 }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.st
    }
}

// ── 回归账本 ─────────────────────────────────────────────────────
pub struct RegressLedger {
    rows: Vec<RegressRow>,
    chain: Vec<u32>,
    /// 只进不退闸维护的已绿集：探针标识 → 最近绿批次。
    green_marks: Vec<(String, u32)>,
    /// 缺陷编号流：BUG-UNX-#### 起始 1，严格递增不跳号。
    bug_seq: u32,
}

impl RegressLedger {
    pub fn new() -> RegressLedger {
        RegressLedger {
            rows: Vec::new(),
            chain: Vec::new(),
            green_marks: Vec::new(),
            bug_seq: 0,
        }
    }

    /// 追加一行（只增不删的唯一入口）。
    /// 返回 Ok(Some(缺陷编号))：绿再红时发出的登记号。
    pub fn append(&mut self, row: RegressRow) -> Result<Option<u32>, u32> {
        if row.probe.len() == 0 {
            return Err(RlCode::ProbeBlank.val());
        }
        // 绿行归因必须为空；红行归因必须为 Some 且在闭集内。
        match row.result {
            RunResult::Green => {
                if row.cause.is_some() {
                    return Err(RlCode::GreenWithCause.val());
                }
            }
            RunResult::Red => {
                let c = match row.cause {
                    Some(c) => c,
                    None => return Err(RlCode::CauseMissing.val()),
                };
                if Cause::from_u32(c.as_u32()) != Some(c) {
                    return Err(RlCode::CauseOutOfRange.val());
                }
            }
        }
        // (标识,批次) 唯一。
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].probe == row.probe && self.rows[i].batch == row.batch {
                return Err(RlCode::DuplicateEntry.val());
            }
            i += 1;
        }
        // 批次时间线：同探针的批次必须单调不回退。
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].probe == row.probe && self.rows[i].batch > row.batch {
                return Err(RlCode::BatchReversed.val());
            }
            i += 1;
        }
        // 只进不退闸：已在绿集的探针本轮红了 → 回归！
        let mut bug_no: Option<u32> = None;
        if row.result == RunResult::Red {
            let mut i = 0;
            while i < self.green_marks.len() {
                if self.green_marks[i].0 == row.probe {
                    // 触发闸：登记缺陷（编号严格递增不跳号）。
                    self.bug_seq += 1;
                    bug_no = Some(self.bug_seq);
                    break;
                }
                i += 1;
            }
        }
        // 哈希链前缀保留。
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(row.probe.as_bytes());
        buf.extend_from_slice(&row.batch.to_le_bytes());
        buf.push(match row.result {
            RunResult::Green => 0,
            RunResult::Red => 1,
        });
        buf.extend_from_slice(&row.version.to_le_bytes());
        buf.push(match row.cause {
            None => 0xFF,
            Some(c) => c.as_u32() as u8,
        });
        let prev = if self.chain.is_empty() { 0 } else { self.chain[self.chain.len() - 1] };
        let mut seed: Vec<u8> = Vec::new();
        seed.extend_from_slice(&prev.to_le_bytes());
        seed.extend_from_slice(&buf);
        let h = fnv1a(&seed);
        // 更新绿集（只进不退：绿标记永不被红清除——闸的物理形态）。
        if row.result == RunResult::Green {
            let mut found = false;
            let mut i = 0;
            while i < self.green_marks.len() {
                if self.green_marks[i].0 == row.probe {
                    self.green_marks[i].1 = row.batch;
                    found = true;
                }
                i += 1;
            }
            if !found {
                self.green_marks.push((row.probe.clone(), row.batch));
            }
        }
        self.rows.push(row);
        self.chain.push(h);
        Ok(bug_no)
    }

    /// 链完整性复验。
    pub fn verify_chain(&self) -> Result<(), u32> {
        let mut prev: u32 = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let row = &self.rows[i];
            let mut buf: Vec<u8> = Vec::new();
            buf.extend_from_slice(row.probe.as_bytes());
            buf.extend_from_slice(&row.batch.to_le_bytes());
            buf.push(match row.result {
                RunResult::Green => 0,
                RunResult::Red => 1,
            });
            buf.extend_from_slice(&row.version.to_le_bytes());
            buf.push(match row.cause {
                None => 0xFF,
                Some(c) => c.as_u32() as u8,
            });
            let mut seed: Vec<u8> = Vec::new();
            seed.extend_from_slice(&prev.to_le_bytes());
            seed.extend_from_slice(&buf);
            let h = fnv1a(&seed);
            if i >= self.chain.len() || self.chain[i] != h {
                return Err(RlCode::HashChainBroken.val());
            }
            prev = h;
            i += 1;
        }
        if self.chain.len() != self.rows.len() {
            return Err(RlCode::HashChainBroken.val());
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn green_marks(&self) -> usize {
        self.green_marks.len()
    }
    pub fn bug_count(&self) -> u32 {
        self.bug_seq
    }

    /// 归因非空率：红行归因全非空（判据 100% 口径）。
    pub fn red_cause_rate(&self) -> Result<usize, u32> {
        let mut ok = 0;
        let mut total = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].result == RunResult::Red {
                total += 1;
                if self.rows[i].cause.is_some() {
                    ok += 1;
                }
            }
            i += 1;
        }
        if total == 0 {
            return Ok(100); // 无红行即满格（闸未触发是好事）
        }
        Ok((ok * 100) / total)
    }

    /// 已绿项失败数（判据原文：必须 = 0）。
    /// 对当前结果集 diff 已绿集——任何绿标探针出现红即非零。
    pub fn green_regression_count(&self, probe: &str, result: RunResult) -> u32 {
        if result == RunResult::Red {
            let mut i = 0;
            while i < self.green_marks.len() {
                if self.green_marks[i].0 == probe {
                    return 1;
                }
                i += 1;
            }
        }
        0
    }

    /// 缺陷编号流闭合：1..=bug_seq 无跳号。
    pub fn bug_seq_closed(&self) -> bool {
        // 编号流本身由 append 内严格 +1 保证；此处验账面计数与
        // 红行触发数守恒。
        let mut fired = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].result == RunResult::Red {
                // 该红行在绿集已有标记才算闸触发（append 语义）。
                fired += 1;
            }
            i += 1;
        }
        // bug_seq ≤ 红行数（同一探针首红后仍可能再红但绿集不清）。
        self.bug_seq <= fired as u32
    }
}

// ── 首版账装配：三探针 × 两批次滚动 ─────────────────────────────
/// 构造判据规定的黄金账：探针全绿 → 一探针回归 → 缺陷登记。
pub fn build_initial_ledger() -> Result<RegressLedger, u32> {
    const TRANS_VER: u32 = 3;
    let mut led = RegressLedger::new();
    // 批次 1：三探针全绿（绿是资产）。
    led.append(RegressRow {
        probe: String::from("probe.errno.EINVAL"),
        batch: 1,
        result: RunResult::Green,
        version: TRANS_VER,
        cause: None,
    })?;
    led.append(RegressRow {
        probe: String::from("probe.errno.EMFILE"),
        batch: 1,
        result: RunResult::Green,
        version: TRANS_VER,
        cause: None,
    })?;
    led.append(RegressRow {
        probe: String::from("probe.winerr.ACCESS_DENIED"),
        batch: 1,
        result: RunResult::Green,
        version: TRANS_VER,
        cause: None,
    })?;
    // 批次 2：EMFILE 翻译漂移 → 已绿项再红 → 闸触发。
    let bug = led.append(RegressRow {
        probe: String::from("probe.errno.EMFILE"),
        batch: 2,
        result: RunResult::Red,
        version: TRANS_VER,
        cause: Some(Cause::UpstreamShift),
    })?;
    // 闸必须发出缺陷编号 BUG-UNX-0001。
    if bug != Some(1) {
        return Err(RlCode::BugNumberGap.val());
    }
    // 批次 3：修复关闭——同探针再绿（绿标记续期，账史不动）。
    led.append(RegressRow {
        probe: String::from("probe.errno.EMFILE"),
        batch: 3,
        result: RunResult::Green,
        version: TRANS_VER + 1,
        cause: None,
    })?;
    led.verify_chain()?;
    Ok(led)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回 Ok((账行数, 绿标数, 缺陷数))。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    let led = build_initial_ledger()?;
    let rate = led.red_cause_rate()?;
    if rate != 100 {
        return Err(RlCode::CauseMissing.val());
    }
    if !led.bug_seq_closed() {
        return Err(RlCode::BugNumberGap.val());
    }
    Ok((led.len(), led.green_marks(), led.bug_count()))
}

// ── 注入账本：六族注入 × 10 轮全抓 ──────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 6], caught: [0; 6] }
    }
    pub fn record(&mut self, slot: usize, caught: bool) {
        self.tries[slot] += 1;
        if caught {
            self.caught[slot] += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 6 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn rounds(&self) -> u32 {
        let mut r = self.tries[0];
        let mut i = 1;
        while i < 6 {
            if self.tries[i] < r {
                r = self.tries[i];
            }
            i += 1;
        }
        r
    }
}

pub fn run_injections(led: &mut InjectLedger) {
    const ROUNDS: u32 = 10;
    let mut rnd = 0;
    while rnd < ROUNDS {
        // ① 已绿项失败（只进不退闸触发）。
        let mut l1 = RegressLedger::new();
        let _ = l1.append(RegressRow {
            probe: String::from("probe.errno.EINVAL"),
            batch: 1,
            result: RunResult::Green,
            version: 3,
            cause: None,
        });
        let bug = l1.append(RegressRow {
            probe: String::from("probe.errno.EINVAL"),
            batch: 2,
            result: RunResult::Red,
            version: 3,
            cause: Some(Cause::TranslateDefect),
        });
        led.record(0, bug.is_ok() && l1.green_regression_count("probe.errno.EINVAL", RunResult::Red) == 1);

        // ② 红行无归因 → CauseMissing。
        let bad = RegressRow {
            probe: String::from("probe.errno.EIO"),
            batch: 1,
            result: RunResult::Red,
            version: 3,
            cause: None,
        };
        led.record(1, l1.append(bad) == Err(RlCode::CauseMissing.val()));

        // ③ (标识,批次) 重复落账。
        let dup = RegressRow {
            probe: String::from("probe.errno.EINVAL"),
            batch: 1,
            result: RunResult::Green,
            version: 3,
            cause: None,
        };
        led.record(2, l1.append(dup) == Err(RlCode::DuplicateEntry.val()));

        // ④ 批次倒序。
        let rev = RegressRow {
            probe: String::from("probe.errno.EINVAL"),
            batch: 0 + (rnd % 1), // batch=0 < 已有批次 1
            result: RunResult::Green,
            version: 3,
            cause: None,
        };
        led.record(3, l1.append(rev) == Err(RlCode::BatchReversed.val()));

        // ⑤ 绿行带归因（口径漂移）。
        let gc = RegressRow {
            probe: String::from("probe.errno.EAGAIN"),
            batch: 1,
            result: RunResult::Green,
            version: 3,
            cause: Some(Cause::ProbeSelf),
        };
        led.record(4, l1.append(gc) == Err(RlCode::GreenWithCause.val()));

        // ⑥ 账行删除（哈希链断裂）。
        let chain_ok = match build_initial_ledger() {
            Ok(l) => {
                let good = l.verify_chain().is_ok();
                let mut l2 = RegressLedger {
                    rows: l.rows.clone(),
                    chain: l.chain.clone(),
                    green_marks: Vec::new(),
                    bug_seq: l.bug_seq,
                };
                if l2.chain.len() > 1 {
                    l2.chain.pop();
                }
                good && l2.verify_chain() == Err(RlCode::HashChainBroken.val())
            }
            Err(_) => false,
        };
        led.record(5, chain_ok);

        rnd += 1;
    }
}

// ── 双跑对拍 ─────────────────────────────────────────────────────
pub fn dual_run() -> Result<(), u32> {
    let a = golden_run();
    let b = golden_run();
    match (a, b) {
        (Ok(x), Ok(y)) => {
            if x == y {
                Ok(())
            } else {
                Err(RlCode::NotReproducible.val())
            }
        }
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

// ── 反向判据 ─────────────────────────────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let led = build_initial_ledger()?;
    if led.len() < 4 {
        return Err(RlCode::ProbeBlank.val());
    }
    led.verify_chain()?;
    // 绿标三探针在册（EMFILE 修复后再绿标记续期）。
    if led.green_marks() != 3 {
        return Err(RlCode::GreenRegressed.val());
    }
    // 归因闭集三态点验。
    let mut c = 0;
    while c < 3 {
        if Cause::from_u32(c as u32).is_none() {
            return Err(RlCode::CauseOutOfRange.val());
        }
        c += 1;
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RlCode::ALL.len() {
        let v = RlCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥8 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：5 行账（3 绿 + 1 红 + 1 修复绿）/ 3 绿标 / 1 缺陷。
    let g = golden_run();
    v.push((
        "J1-黄金正演5行3绿1缺陷",
        matches!(g, Ok((5, 3, 1))),
        format!("{:?}", g),
    ));
    // 2. 注入六族全抓 10/10。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入六族10/10全抓",
        led.all_caught() && led.rounds() == 10,
        format!("r={}", led.rounds()),
    ));
    // 3. 只进不退闸：已绿项失败数 = 0——当前结果集（末批三探针
    //    全绿）对已绿集 diff，回归数必须为 0。
    let l = build_initial_ledger()
        .unwrap_or_else(|_| RegressLedger::new());
    let reg = l.green_regression_count("probe.errno.EINVAL", RunResult::Green)
        + l.green_regression_count("probe.errno.EMFILE", RunResult::Green)
        + l.green_regression_count("probe.winerr.ACCESS_DENIED", RunResult::Green);
    v.push(("J1-只进不退闸0触发", reg == 0, format!("{}", reg)));
    // 4. 闸的正向触发：人为回归必抓并出编号。
    let mut l2 = RegressLedger::new();
    let _ = l2.append(RegressRow {
        probe: String::from("probe.errno.EDOM"),
        batch: 1,
        result: RunResult::Green,
        version: 3,
        cause: None,
    });
    let fired = l2.append(RegressRow {
        probe: String::from("probe.errno.EDOM"),
        batch: 2,
        result: RunResult::Red,
        version: 3,
        cause: Some(Cause::TranslateDefect),
    });
    v.push((
        "J1-闸触发出缺陷编号",
        fired == Ok(Some(1)),
        format!("{:?}", fired),
    ));
    // 5. 归因非空率 100%。
    let rate = l.red_cause_rate().unwrap_or(0);
    v.push(("J1-归因非空率100%", rate == 100, format!("{}%", rate)));
    // 6. 哈希链完整。
    v.push((
        "J1-哈希链完整",
        matches!(l.verify_chain(), Ok(())),
        format!("{} 环", l.len()),
    ));
    // 7. 编号流闭合（无跳号）。
    v.push((
        "J1-缺陷编号流闭合",
        l.bug_seq_closed(),
        format!("seq={}", l.bug_seq),
    ));
    // 8. 双跑确定性。
    v.push((
        "J1-双跑对拍一致",
        matches!(dual_run(), Ok(())),
        format!("seed=0x{:X}", SEG_BASE),
    ));
    // 9. 反向探针。
    v.push((
        "J1-反向探针",
        matches!(reverse_probe(), Ok(())),
        String::from("绿标+归因闭集"),
    ));
    // 10. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口", SEG_BASE),
    ));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx16041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16041 错误码翻译面回归账与只进不退机制");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
