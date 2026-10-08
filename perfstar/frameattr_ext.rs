//! F042 帧率归因器 · 深化件（AI-K1 深化批次三 · G-B-02）。
//!
//! 主册判据里「说到了但没落到数据结构」的五处，本件逐条实装：
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【交互设计】「点开单条看四类嫌疑占比条形图 + **每类的原始证据（哪次 IO/哪个窗口多大脏区）**」 | [`EvidenceRef`] 证据引用（引用不复制，一处一事实）+ [`AttrCase`] 案例 |
//! | 2 | 【交互设计】「列表（**时间/掉帧数/头号归因/证据链接**）」 | [`HistoryRow`] 掉帧历史列表行 |
//! | 3 | 【数据与存储】「归因事件落账本分钟聚合……**保留 7 天**」 | [`CaseBook`] 定长案例簿 + [`Retention`] 7 天轮转 |
//! | 4 | 【状态与异常】「**归因器自身异常 → 静默停用 + 诊断报备（不拖累合成器）**」 | [`AttributorHealth`] 健康状态机：连续异常 → 停用（只停产出不停观测）+ 报备 + 恢复 |
//! | 5 | 【验收判据】「**误报率 <10%**（正常拖动 100 秒样本零误报）」 | [`FalsePositiveMeter`] 误报率账（样本/误报/千分率 + 100 秒窗判定） |
//!
//! 与主域的分工：主域 `frameattr` 负责「四类嫌疑的贡献模型与判定」，
//! 本件负责「判据要求的数据组织、保留、健康与误报账」——不重复实现判定。

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink, Retention, RetentionVerdict};

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 四类嫌疑（主册【设计细节】四类判定阈值，编号即条形图顺序，顺序冻结）。
pub const SUSPECTS: usize = 4;
/// 案例簿容量（定长；7 天 × 每分钟最多一条的保守上界远大于此，环满覆盖最旧）。
pub const CASE_CAP: usize = 64;
/// 保留期 7 天（主册【数据与存储】「保留 7 天」）。
pub const KEEP_MS: u64 = 7 * 24 * 3_600 * 1_000;
/// 正常拖动误报观察窗 100 秒（主册【验收判据】）。
pub const FP_WINDOW_MS: u64 = 100_000;
/// 误报率红线 10%（千分 = 100）。
pub const FP_REDLINE_PERMILLE: u32 = 100;
/// 连续自身异常触发停用的次数（不因一次抖动就停用归因能力）。
pub const DISABLE_STREAK: u32 = 3;
/// 停用后恢复所需的连续健康次数（迟滞，防抖）。
pub const RESTORE_STREAK: u32 = 30;

/// 嫌疑编号（与主域贡献数组下标一致，一处一事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Suspect {
    /// 输入风暴（输入事件 >200/秒）。
    InputStorm = 0,
    /// 大脏区（单帧脏区 >屏幕 60%）。
    BigDirty = 1,
    /// IO 阻塞（帧内 IO 等待 >2ms）。
    IoBlock = 2,
    /// 调度抢占（帧被高优先线程抢占 >3 次）。
    Preempt = 3,
}

impl Suspect {
    pub const fn name(self) -> &'static str {
        match self {
            Suspect::InputStorm => "input-storm",
            Suspect::BigDirty => "big-dirty",
            Suspect::IoBlock => "io-block",
            Suspect::Preempt => "preempt",
        }
    }
    pub const fn threshold_text(self) -> &'static str {
        match self {
            Suspect::InputStorm => "输入事件 >200/秒",
            Suspect::BigDirty => "单帧脏区 >屏幕 60%",
            Suspect::IoBlock => "帧内 IO 等待 >2ms",
            Suspect::Preempt => "帧被高优先线程抢占 >3 次",
        }
    }
}

// ---------------------------------------------------------------------------
// 1. 证据引用（不复制，一处一事实）
// ---------------------------------------------------------------------------

/// 证据种类：引用哪个账本的哪一条（引用 = 索引 + 时刻，不搬数据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceRef {
    /// 无证据（诚实：多因并发或采集失败时就是没有，不编一个）。
    None,
    /// IO 请求：引用 IO 账本请求序号。
    IoRequest { req_id: u64, wait_us: u32 },
    /// 某个窗口的大脏区：窗口 id + 脏区千分屏。
    DirtyWindow { window_id: u32, dirty_permille: u16 },
    /// 输入风暴：该秒输入事件数。
    InputBurst { events_per_sec: u32 },
    /// 调度抢占：抢占次数 + 抢占线程优先级。
    Preemption { count: u32, by_prio: u8 },
}

impl EvidenceRef {
    /// 该证据是否构成「可点开看」的实体（None 时呈现面显示为「无原始证据」）。
    pub fn is_concrete(&self) -> bool {
        !matches!(self, EvidenceRef::None)
    }
    /// 判定该证据是否达到主册阈值（呈现面据此标注「达标/未达标」）。
    pub fn meets_threshold(&self) -> bool {
        match *self {
            EvidenceRef::None => false,
            EvidenceRef::IoRequest { wait_us, .. } => wait_us > 2_000,
            EvidenceRef::DirtyWindow { dirty_permille, .. } => dirty_permille > 600,
            EvidenceRef::InputBurst { events_per_sec } => events_per_sec > 200,
            EvidenceRef::Preemption { count, .. } => count > 3,
        }
    }
}

/// 一个归因案例（点开单条看到的东西：四类占比 + 每类证据 + 头号归因）。
#[derive(Clone, Copy, Debug)]
pub struct AttrCase {
    pub at_ms: u64,
    /// 该次卡顿掉帧数。
    pub dropped_frames: u32,
    /// 四类贡献（千分，和恒 1000，主域 `shares_permille` 产出）。
    pub shares_permille: [u16; SUSPECTS],
    /// 头号归因（None = 未归类，主域诚实空图口径）。
    pub top: Option<Suspect>,
    /// 每类的原始证据（下标与主域贡献数组一致）。
    pub evidence: [EvidenceRef; SUSPECTS],
    /// 证据引用指向的账本帧序号（引用不复制）。
    pub ledger_frame_seq: u64,
}

impl AttrCase {
    /// 头号归因下标（并列取编号小者，确定性优先——不引入随机）。
    pub fn top_index(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..SUSPECTS {
            if self.shares_permille[i] == 0 {
                continue;
            }
            match best {
                None => best = Some(i),
                Some(b) if self.shares_permille[i] > self.shares_permille[b] => best = Some(i),
                _ => {}
            }
        }
        best
    }
    /// 是否为混合归因（证据不足时如实标注，主册「不硬编主因」）。
    ///
    /// 判据：头号占比**未过半**（≤500‰）即混合——正好五五开也算混合，
    /// 两个各占一半的嫌疑没有任何理由指定其中一个是主因。
    pub fn is_mixed(&self) -> bool {
        match self.top_index() {
            Some(i) => self.shares_permille[i] <= 500,
            None => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 2. 掉帧历史列表行（交互设计四列）
// ---------------------------------------------------------------------------

/// 历史列表一行：时间 / 掉帧数 / 头号归因 / 证据链接。
#[derive(Clone, Copy, Debug)]
pub struct HistoryRow {
    pub at_ms: u64,
    pub dropped_frames: u32,
    pub top_index: Option<u8>,
    /// 案例簿下标（证据链接 = 指向案例，不复制案例）。
    pub case_link: u16,
}

/// 案例簿：定长环 + 7 天保留（覆盖最旧，过期优先）。
pub struct CaseBook {
    cases: [Option<AttrCase>; CASE_CAP],
    head: usize,
    filled: usize,
    retention: Retention,
    /// 覆盖丢弃计数（环满）。
    pub overwritten: u64,
}

impl CaseBook {
    pub const fn new() -> Self {
        CaseBook {
            cases: [None; CASE_CAP],
            head: 0,
            filled: 0,
            retention: Retention::new(CASE_CAP as u32, KEEP_MS),
            overwritten: 0,
        }
    }

    /// 登记一个案例。`now_ms` 用于保留期裁定。
    pub fn push(&mut self, case: AttrCase, now_ms: u64) {
        let oldest = self.oldest_ms().unwrap_or(now_ms);
        match self.retention.admit(now_ms, self.filled as u32, oldest) {
            RetentionVerdict::Evict(_) | RetentionVerdict::Expire(_) => self.overwritten += 1,
            RetentionVerdict::Admit => {}
        }
        self.cases[self.head] = Some(case);
        self.head = (self.head + 1) % CASE_CAP;
        self.filled = (self.filled + 1).min(CASE_CAP);
    }

    fn oldest_ms(&self) -> Option<u64> {
        if self.filled == 0 {
            return None;
        }
        let start = (self.head + CASE_CAP - self.filled) % CASE_CAP;
        self.cases[start].map(|c| c.at_ms)
    }

    /// 历史列表导出（时间升序）。
    pub fn history(&self, out: &mut [HistoryRow]) -> usize {
        let n = self.filled.min(out.len());
        let start = (self.head + CASE_CAP - n) % CASE_CAP;
        for i in 0..n {
            let idx = (start + i) % CASE_CAP;
            if let Some(c) = self.cases[idx] {
                out[i] = HistoryRow {
                    at_ms: c.at_ms,
                    dropped_frames: c.dropped_frames,
                    top_index: c.top_index().map(|v| v as u8),
                    case_link: idx as u16,
                };
            }
        }
        n
    }

    /// 按链接取案例（引用返回，不复制）。
    pub fn case_at(&self, link: u16) -> Option<&AttrCase> {
        let i = link as usize;
        if i < CASE_CAP {
            self.cases[i].as_ref()
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.filled
    }
    pub fn expired(&self) -> u64 {
        self.retention.expired
    }
    pub fn evicted(&self) -> u64 {
        self.retention.evicted
    }
}

// ---------------------------------------------------------------------------
// 3. 归因器健康（状态与异常：静默停用 + 诊断报备，不拖累合成器）
// ---------------------------------------------------------------------------

/// 归因器运行状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// 正常产出归因。
    Active,
    /// 静默停用（只停产出，不停观测——与 F049「停用态照收」同纪律）。
    Disabled,
}

/// 归因器健康状态机：连续异常 → 停用 + 报备；连续健康 → 恢复。
///
/// 主册原文「归因器自身异常 → 静默停用 + 诊断报备（不拖累合成器）」——
/// 「不拖累」是本件的设计约束：停用期间不得再调用判定路径（零开销），
/// 但证据收集路径照常（恢复后才能给出完整归因，不留空窗）。
pub struct AttributorHealth {
    state: Health,
    bad_streak: u32,
    ok_streak: u32,
    /// 停用次数（审计）。
    pub disabled_times: u32,
    /// 停用期间被跳过的归因请求数（诚实计数：能力缺失有过代价）。
    pub skipped_requests: u64,
    /// 最近一次停用时刻。
    pub last_disabled_ms: Option<u64>,
}

impl AttributorHealth {
    pub const fn new() -> Self {
        AttributorHealth {
            state: Health::Active,
            bad_streak: 0,
            ok_streak: 0,
            disabled_times: 0,
            skipped_requests: 0,
            last_disabled_ms: None,
        }
    }

    pub fn state(&self) -> Health {
        self.state
    }

    /// 是否应产出归因（停用 = 不产出）。
    pub fn should_attribute(&self) -> bool {
        self.state == Health::Active
    }

    /// 上报一次归因器自身执行结果。`sink` 非空时停用/恢复事件入诊断报备。
    pub fn report(&mut self, ok: bool, now_ms: u64, sink: Option<&mut DiagSink>) {
        if ok {
            self.bad_streak = 0;
            self.ok_streak = self.ok_streak.saturating_add(1);
            if self.state == Health::Disabled && self.ok_streak >= RESTORE_STREAK {
                self.state = Health::Active;
                self.ok_streak = 0;
                if let Some(s) = sink {
                    s.push("F042", 2, now_ms, DiagSev::Info, 0, 0, b"attributor restored");
                }
            }
        } else {
            self.ok_streak = 0;
            self.bad_streak = self.bad_streak.saturating_add(1);
            if self.state == Health::Active && self.bad_streak >= DISABLE_STREAK {
                self.state = Health::Disabled;
                self.bad_streak = 0;
                self.disabled_times += 1;
                self.last_disabled_ms = Some(now_ms);
                if let Some(s) = sink {
                    // 停用不静默：进诊断报备（用户/开发者查得到）
                    s.push("F042", 1, now_ms, DiagSev::Error, 0, 0, b"attributor disabled");
                }
            }
        }
    }

    /// 停用期间请求归因：只计数，不执行（零开销，不拖累合成器）。
    pub fn skip_one(&mut self) {
        if self.state == Health::Disabled {
            self.skipped_requests = self.skipped_requests.saturating_add(1);
        }
    }
}

// ---------------------------------------------------------------------------
// 4. 误报率账（验收判据：正常拖动 100 秒样本零误报）
// ---------------------------------------------------------------------------

/// 误报率账：误报 = 在正常（无掉帧）样本上给出了归因结论。
///
/// 主册把「误报率 <10%」写成判据，但没有给它数据结构——判据不可测就只是
/// 口号。本件把「正常拖动 100 秒」做成可执行的窗口判定。
#[derive(Clone, Copy, Debug)]
pub struct FalsePositiveMeter {
    /// 观察窗起点（None = 未开窗）。
    window_start_ms: Option<u64>,
    /// 窗内正常样本数（无掉帧的帧）。
    pub samples: u32,
    /// 窗内被判为「有归因结论」的正常样本数（= 误报）。
    pub false_positives: u32,
    /// 已完成的最长一次观察窗时长（毫秒）。
    pub longest_window_ms: u64,
}

impl FalsePositiveMeter {
    pub const fn new() -> Self {
        FalsePositiveMeter { window_start_ms: None, samples: 0, false_positives: 0, longest_window_ms: 0 }
    }

    /// 开/续窗。同一场景连续喂样本；场景切换调用 [`FalsePositiveMeter::close`]。
    pub fn open(&mut self, now_ms: u64) {
        if self.window_start_ms.is_none() {
            self.window_start_ms = Some(now_ms);
        }
    }

    /// 喂一个样本：`dropped` = 该帧真的掉了帧；`attributed` = 归器给了结论。
    /// 只有「没掉帧却给了结论」才算误报——掉了帧给结论是本职工作。
    pub fn feed(&mut self, dropped: bool, attributed: bool) {
        self.samples = self.samples.saturating_add(1);
        if !dropped && attributed {
            self.false_positives = self.false_positives.saturating_add(1);
        }
    }

    /// 误报率（千分）。零样本返回 0 并不可与「零误报」混淆——
    /// 用 [`FalsePositiveMeter::samples`] 判定是否真的跑过窗。
    pub fn permille(&self) -> u32 {
        if self.samples == 0 {
            return 0;
        }
        ((self.false_positives as u64 * 1000) / self.samples as u64) as u32
    }

    /// 是否满足判据（窗内样本足够 + 误报率低于红线）。
    pub fn passes(&self) -> bool {
        self.samples > 0 && self.permille() < FP_REDLINE_PERMILLE
    }

    /// 关窗（记录窗长，重置计数）。返回本次窗是否达标。
    pub fn close(&mut self, now_ms: u64) -> bool {
        let ok = self.passes();
        if let Some(s) = self.window_start_ms {
            let len = now_ms.saturating_sub(s);
            if len > self.longest_window_ms {
                self.longest_window_ms = len;
            }
        }
        self.window_start_ms = None;
        self.samples = 0;
        self.false_positives = 0;
        ok
    }

    /// 窗长是否达到 100 秒判据要求（「正常拖动 100 秒样本」的可执行口径）。
    pub fn window_long_enough(&self) -> bool {
        self.longest_window_ms >= FP_WINDOW_MS
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化件检查项）
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F042-frameattr-ext");
    // 1) 四类嫌疑名称与阈值文案取自同一枚举（一处一事实，不复制字符串）。
    cs.add(
        "suspects_named_and_thresholded",
        Suspect::InputStorm.name() == "input-storm"
            && Suspect::BigDirty.threshold_text() == "单帧脏区 >屏幕 60%"
            && Suspect::IoBlock.threshold_text() == "帧内 IO 等待 >2ms"
            && Suspect::Preempt.threshold_text() == "帧被高优先线程抢占 >3 次",
        "",
    );
    // 2) 证据引用不复制：只带索引与数值。
    let ev = EvidenceRef::IoRequest { req_id: 7, wait_us: 2_500 };
    cs.add("evidence_is_reference", ev.is_concrete() && ev.meets_threshold(), "");
    let ev2 = EvidenceRef::IoRequest { req_id: 8, wait_us: 1_000 };
    cs.add("evidence_below_threshold_not_counted", ev2.is_concrete() && !ev2.meets_threshold(), "");
    cs.add("evidence_none_is_honest", !EvidenceRef::None.is_concrete() && !EvidenceRef::None.meets_threshold(), "");
    // 3) 头号归因 = 占比最大者；并列取编号小者（确定性）。
    let mut c = AttrCase {
        at_ms: 1_000,
        dropped_frames: 3,
        shares_permille: [100, 300, 500, 100],
        top: None,
        evidence: [EvidenceRef::None; SUSPECTS],
        ledger_frame_seq: 11,
    };
    cs.add("top_is_max_share", c.top_index() == Some(2), "");
    // 4) 混合归因：头号未过半 → 如实标注，不硬编主因。
    cs.add("mixed_when_no_majority", c.is_mixed(), "");
    c.shares_permille = [0, 0, 900, 100];
    cs.add("single_cause_not_mixed", !c.is_mixed(), "");
    // 5) 未归类（全零占比）→ top 为 None，呈现面诚实空图。
    c.shares_permille = [0; SUSPECTS];
    cs.add("unclassified_is_none", c.top_index().is_none() && c.is_mixed(), "");
    // 6) 案例簿 7 天保留 + 环满覆盖计数。
    let mut book = CaseBook::new();
    for i in 0..(CASE_CAP + 5) {
        book.push(
            AttrCase {
                at_ms: 1_000 + i as u64,
                dropped_frames: 1,
                shares_permille: [250, 250, 250, 250],
                top: Some(Suspect::InputStorm),
                evidence: [EvidenceRef::None; SUSPECTS],
                ledger_frame_seq: i as u64,
            },
            1_000 + i as u64,
        );
    }
    cs.add("casebook_covers_oldest", book.len() == CASE_CAP && book.overwritten == 5, "");
    // 7) 历史列表四列齐全（时间/掉帧数/头号归因/证据链接）。
    let mut rows = [HistoryRow { at_ms: 0, dropped_frames: 0, top_index: None, case_link: 0 }; 8];
    let n = book.history(&mut rows);
    let linked = book.case_at(rows[n - 1].case_link);
    cs.add("history_row_complete", n == 8 && rows[0].dropped_frames == 1 && rows[0].top_index == Some(0) && linked.is_some(), "");
    // 8) 健康状态机：连续 3 次异常 → 停用 + 诊断报备（不静默）。
    let mut sink = DiagSink::new();
    let mut h = AttributorHealth::new();
    h.report(false, 1_000, Some(&mut sink));
    h.report(false, 2_000, Some(&mut sink));
    let still_active = h.state() == Health::Active;
    h.report(false, 3_000, Some(&mut sink));
    cs.add(
        "health_disables_and_reports",
        still_active && h.state() == Health::Disabled && h.disabled_times == 1 && sink.count(DiagSev::Error) == 1,
        "",
    );
    // 9) 停用期间不产出但照常计数（不拖累合成器 + 代价可查）。
    h.skip_one();
    h.skip_one();
    cs.add("disabled_skips_counted", !h.should_attribute() && h.skipped_requests == 2, "");
    // 10) 恢复迟滞：连续 30 次健康才恢复（防抖）。
    let mut restored_at = None;
    for i in 0..RESTORE_STREAK {
        h.report(true, 10_000 + i as u64, Some(&mut sink));
        if h.state() == Health::Active && restored_at.is_none() {
            restored_at = Some(i);
        }
    }
    cs.add("health_restores_after_streak", restored_at == Some(RESTORE_STREAK - 1) && h.state() == Health::Active, "");
    // 11) 误报率账：正常样本给了结论才算误报。
    let mut m = FalsePositiveMeter::new();
    m.open(0);
    for _ in 0..100 {
        m.feed(false, false); // 正常帧，无结论 → 不算误报
    }
    cs.add("fp_zero_on_clean_samples", m.permille() == 0 && m.passes(), "");
    for _ in 0..5 {
        m.feed(false, true); // 正常帧却给了结论 → 误报
    }
    cs.add("fp_counts_only_wrong_conclusions", m.false_positives == 5 && m.permille() == 47, "");
    // 掉帧帧给了结论不是误报
    let mut m2 = FalsePositiveMeter::new();
    m2.open(0);
    for _ in 0..10 {
        m2.feed(true, true);
    }
    cs.add("fp_true_drop_attributed_is_correct", m2.false_positives == 0, "");
    // 12) 100 秒窗判定（判据「正常拖动 100 秒样本」的可执行口径）。
    let mut m3 = FalsePositiveMeter::new();
    m3.open(0);
    for _ in 0..6000 {
        m3.feed(false, false);
    }
    let ok = m3.close(100_000);
    cs.add("fp_window_100s", ok && m3.window_long_enough(), "");
    cs
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_none_never_fabricated() {
        // 多因并发且采集失败时：证据就是 None，不给「看起来有」的占位证据
        let c = AttrCase {
            at_ms: 0,
            dropped_frames: 1,
            shares_permille: [250, 250, 250, 250],
            top: None,
            evidence: [EvidenceRef::None; SUSPECTS],
            ledger_frame_seq: 0,
        };
        assert!(c.evidence.iter().all(|e| !e.is_concrete()));
    }

    #[test]
    fn casebook_history_is_time_ascending() {
        let mut b = CaseBook::new();
        for i in 0..5u64 {
            b.push(
                AttrCase {
                    at_ms: i * 1_000,
                    dropped_frames: i as u32,
                    shares_permille: [1000, 0, 0, 0],
                    top: Some(Suspect::InputStorm),
                    evidence: [EvidenceRef::None; SUSPECTS],
                    ledger_frame_seq: i,
                },
                i * 1_000,
            );
        }
        let mut rows = [HistoryRow { at_ms: 0, dropped_frames: 0, top_index: None, case_link: 0 }; 5];
        b.history(&mut rows);
        assert_eq!(rows[0].at_ms, 0);
        assert_eq!(rows[4].at_ms, 4_000);
    }

    #[test]
    fn health_does_not_disable_on_single_glitch() {
        let mut h = AttributorHealth::new();
        h.report(false, 0, None);
        h.report(true, 1, None);
        h.report(false, 2, None);
        assert_eq!(h.state(), Health::Active, "单次抖动不剥夺归因能力");
    }

    #[test]
    fn retention_seven_days_is_wired() {
        assert_eq!(KEEP_MS, 7 * 24 * 3_600 * 1_000);
        let mut b = CaseBook::new();
        b.push(
            AttrCase { at_ms: 0, dropped_frames: 1, shares_permille: [0; 4], top: None, evidence: [EvidenceRef::None; 4], ledger_frame_seq: 0 },
            0,
        );
        // 8 天后登记：最旧一条已超龄 → 过期计数前进
        let eight_days = 8 * 24 * 3_600 * 1_000;
        b.push(
            AttrCase { at_ms: eight_days, dropped_frames: 1, shares_permille: [0; 4], top: None, evidence: [EvidenceRef::None; 4], ledger_frame_seq: 1 },
            eight_days,
        );
        assert_eq!(b.expired(), 1);
    }

    #[test]
    fn fp_meter_zero_samples_is_not_zero_fp() {
        let m = FalsePositiveMeter::new();
        assert_eq!(m.permille(), 0);
        assert!(!m.passes(), "零样本 ≠ 零误报：没跑过窗不能算达标");
    }
}
