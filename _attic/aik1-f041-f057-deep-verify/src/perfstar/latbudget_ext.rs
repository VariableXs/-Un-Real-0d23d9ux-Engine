//! F047 调度器延迟预算深化 · 深化件（AI-K1 深化批次三 · G-B-07）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【交互设计】「**点击超标点看当时线程与优先级快照**」 | [`ThreadSnapshot`] 超标点可下钻的线程快照（tid/优先级/类别/延迟/是否超预算） |
//! | 2 | 【状态与异常】「**某类长期零样本 → 观察窗说明（不代表无风险）**」 | [`ObservationNote`] 零样本类别的诚实标注（不把「没数据」说成「没问题」） |
//! | 3 | 【状态与异常】「超预算且**归因到自身策略（如优先级饥饿）→ 调度器自修正（优先级老化）并记录修正事件**」 | [`SelfCorrect`] 老化自修正状态机（触发/冷却/升级上限/事件账） |
//! | 4 | 【设计细节】「**优先级映射：输入=最高带抢占、音频=高带 deadline、合成=高、普通=rr 老化**」 | [`PriorityMap`] 优先级规格表（一处一事实，调度器与监视器同读） |
//! | 5 | 【设计细节】「**energy=balanced 策略（既有实测）在大核可用时保持**」 | [`EnergyPolicy`] 大核可用性联动（不可用时的降级与标注） |
//! | 6 | 【数据与存储】「延迟采样每类环形 **10,000 条（内存 ~160KB）**」 | [`RingBudget`] 内存预算自证（4 类 × 10k × 4B = 160KB） |
//! | 7 | 【设计细节】「预算表**编译期常量**（变更走 ADR）」 | [`Budget`] 常量表 + ADR 登记说明 |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink};

// ---------------------------------------------------------------------------
// 常量（主册【功能定义】逐字；变更走 ADR）
// ---------------------------------------------------------------------------

/// 总预算 2000μs（主册【功能定义】）。
pub const TOTAL_BUDGET_US: u32 = 2_000;
/// 输入类子预算 500μs。
pub const BUDGET_INPUT_US: u32 = 500;
/// 合成类子预算 800μs。
pub const BUDGET_COMPOSE_US: u32 = 800;
/// 音频类子预算 300μs。
pub const BUDGET_AUDIO_US: u32 = 300;
/// 普通类子预算 400μs。
pub const BUDGET_NORMAL_US: u32 = 400;
/// 每类环形采样条数 10,000（主册【数据与存储】）。
pub const RING_PER_CLASS: usize = 10_000;
/// 单条采样字节数（u32 延迟值）。
pub const SAMPLE_BYTES: usize = 4;
/// 类别数。
pub const CLASSES: usize = 4;
/// 老化自修正冷却（毫秒）：修正后至少观察这么久再修，防抖。
pub const AGING_COOLDOWN_MS: u64 = 30_000;
/// 老化提升上限（最多提 3 级，超过说明不是优先级问题——停止自修正并报备）。
pub const AGING_MAX_BOOST: u8 = 3;
/// 零样本观察窗（毫秒）：超过这个时长仍零样本才出观察窗说明。
pub const OBSERVE_WINDOW_MS: u64 = 60_000;

/// 延迟类别（与主域四类一致：输入/合成/音频/普通）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Input = 0,
    Compose = 1,
    Audio = 2,
    Normal = 3,
}

impl Class {
    pub const fn name(self) -> &'static str {
        match self {
            Class::Input => "输入",
            Class::Compose => "合成",
            Class::Audio => "音频",
            Class::Normal => "普通",
        }
    }
    pub const fn budget_us(self) -> u32 {
        match self {
            Class::Input => BUDGET_INPUT_US,
            Class::Compose => BUDGET_COMPOSE_US,
            Class::Audio => BUDGET_AUDIO_US,
            Class::Normal => BUDGET_NORMAL_US,
        }
    }
    pub const fn idx(self) -> usize {
        self as usize
    }
}

/// 预算表（编译期常量，主册「变更走 ADR」——本件只做常量集合与自洽校验）。
pub struct Budget;

impl Budget {
    /// 四类子预算之和必须等于总预算（自洽校验：改一处必须改另一处）。
    pub const fn sum_matches_total() -> bool {
        BUDGET_INPUT_US + BUDGET_COMPOSE_US + BUDGET_AUDIO_US + BUDGET_NORMAL_US == TOTAL_BUDGET_US
    }
    /// 按类别取预算。
    pub const fn of(c: Class) -> u32 {
        c.budget_us()
    }
    /// ADR 登记说明（变更入口写在代码里，不靠口头约定）。
    pub const fn adr_note() -> &'static str {
        "预算表为编译期常量，任何变更必须走 ADR 并同步 F061 基准回归门阈值"
    }
}

// ---------------------------------------------------------------------------
// 1. 线程与优先级快照（点击超标点下钻）
// ---------------------------------------------------------------------------

/// 超标点下钻快照（主册「点击超标点看当时线程与优先级快照」）。
#[derive(Clone, Copy, Debug)]
pub struct ThreadSnapshot {
    /// 线程 id。
    pub tid: u32,
    /// 当时优先级（含老化提升后的有效优先级）。
    pub prio: u8,
    /// 所属延迟类别。
    pub class: Class,
    /// 时刻。
    pub at_ms: u64,
    /// 本次唤醒延迟（微秒）。
    pub latency_us: u32,
    /// 是否超本类子预算。
    pub over_budget: bool,
}

impl ThreadSnapshot {
    /// 超预算幅度千分（超出多少——归因要看幅度，不只是「超了」）。
    pub fn over_permille(&self) -> u32 {
        let b = self.class.budget_us();
        if b == 0 || self.latency_us <= b {
            return 0;
        }
        (((self.latency_us - b) as u64 * 1000) / b as u64) as u32
    }
}

/// 快照簿（定长 32 条最近超标点，覆盖最旧）。
pub struct SnapshotBook {
    ring: [Option<ThreadSnapshot>; 32],
    head: usize,
    filled: usize,
    /// 记录总数（含被覆盖的——诚实计数）。
    pub total: u64,
}

impl SnapshotBook {
    pub const fn new() -> Self {
        SnapshotBook { ring: [None; 32], head: 0, filled: 0, total: 0 }
    }
    /// 记录一个超标点（只记超预算的——主域泳道曲线负责全量，本件负责可下钻面）。
    pub fn push(&mut self, s: ThreadSnapshot) {
        if !s.over_budget {
            return;
        }
        self.ring[self.head] = Some(s);
        self.head = (self.head + 1) % 32;
        self.filled = (self.filled + 1).min(32);
        self.total += 1;
    }
    pub fn snapshot(&self, out: &mut [ThreadSnapshot]) -> usize {
        let n = self.filled.min(out.len());
        let start = (self.head + 32 - n) % 32;
        for i in 0..n {
            if let Some(s) = self.ring[(start + i) % 32] {
                out[i] = s;
            }
        }
        n
    }
    pub fn len(&self) -> usize {
        self.filled
    }
}

// ---------------------------------------------------------------------------
// 2. 优先级映射（一处一事实）
// ---------------------------------------------------------------------------

/// 优先级规格（主册【设计细节】映射表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrioritySpec {
    /// 基础优先级（数值小 = 高）。
    pub base: u8,
    /// 是否带抢占。
    pub preempt: bool,
    /// 是否带 deadline。
    pub deadline: bool,
    /// 是否走 rr 老化。
    pub aging: bool,
}

/// 优先级映射表（调度器与监视器同读这一份）。
pub struct PriorityMap;

impl PriorityMap {
    /// 输入 = 最高带抢占。
    const INPUT: PrioritySpec = PrioritySpec { base: 0, preempt: true, deadline: false, aging: false };
    /// 音频 = 高带 deadline。
    const AUDIO: PrioritySpec = PrioritySpec { base: 1, preempt: false, deadline: true, aging: false };
    /// 合成 = 高。
    const COMPOSE: PrioritySpec = PrioritySpec { base: 2, preempt: false, deadline: false, aging: false };
    /// 普通 = rr 老化。
    const NORMAL: PrioritySpec = PrioritySpec { base: 3, preempt: false, deadline: false, aging: true };

    pub const fn of(c: Class) -> PrioritySpec {
        match c {
            Class::Input => Self::INPUT,
            Class::Compose => Self::COMPOSE,
            Class::Audio => Self::AUDIO,
            Class::Normal => Self::NORMAL,
        }
    }
    /// 有效优先级（基础 - 老化提升；不越过 0）。
    pub fn effective(c: Class, boost: u8) -> u8 {
        let base = Self::of(c).base;
        base.saturating_sub(boost)
    }
    /// 是否可抢占（只有输入类带抢占——主册原文）。
    pub const fn is_preemptive(c: Class) -> bool {
        Self::of(c).preempt
    }
    /// 输入类优先级必须严格高于其他三类（交互优先不是口号）。
    pub const fn input_strictly_highest() -> bool {
        Self::INPUT.base < Self::AUDIO.base && Self::INPUT.base < Self::COMPOSE.base && Self::INPUT.base < Self::NORMAL.base
    }
}

// ---------------------------------------------------------------------------
// 3. 老化自修正（归因到自身策略时调度器自修正）
// ---------------------------------------------------------------------------

/// 自修正动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorrectAction {
    /// 不动作（未归因到自身策略，或冷却中）。
    None,
    /// 提升该类优先级（老化）。
    Boost,
    /// 已达提升上限：停止自修正并报备（继续提说明不是优先级问题）。
    GiveUp,
}

/// 老化自修正器：超预算且归因到自身策略（优先级饥饿）时自修正。
///
/// 主册要求「记录修正事件」——本件把每次修正连同事由与冷却都记进诊断报备，
/// 避免「调度器悄悄改了优先级」变成不可解释的行为。
pub struct SelfCorrect {
    boost: [u8; CLASSES],
    last_correct_ms: [Option<u64>; CLASSES],
    /// 各类修正次数。
    pub corrections: [u32; CLASSES],
    /// 因达上限而放弃的次数。
    pub giveups: u32,
    /// 冷却期内被抑制的修正请求数。
    pub cooled: u32,
}

impl SelfCorrect {
    pub const fn new() -> Self {
        SelfCorrect { boost: [0; CLASSES], last_correct_ms: [None; CLASSES], corrections: [0; CLASSES], giveups: 0, cooled: 0 }
    }
    /// 当前提升量。
    pub fn boost(&self, c: Class) -> u8 {
        self.boost[c.idx()]
    }
    /// 请求一次自修正。`self_caused` = 是否归因到自身策略（优先级饥饿）。
    pub fn request(&mut self, c: Class, self_caused: bool, now_ms: u64, sink: Option<&mut DiagSink>) -> CorrectAction {
        if !self_caused {
            return CorrectAction::None;
        }
        let i = c.idx();
        if let Some(t) = self.last_correct_ms[i] {
            if now_ms.saturating_sub(t) < AGING_COOLDOWN_MS {
                self.cooled += 1;
                return CorrectAction::None;
            }
        }
        if self.boost[i] >= AGING_MAX_BOOST {
            self.giveups += 1;
            if let Some(s) = sink {
                s.push("F047", 3, now_ms, DiagSev::Warn, c.idx() as u64, self.boost[i] as u64, b"aging boost capped");
            }
            return CorrectAction::GiveUp;
        }
        self.boost[i] += 1;
        self.corrections[i] += 1;
        self.last_correct_ms[i] = Some(now_ms);
        if let Some(s) = sink {
            s.push("F047", 2, now_ms, DiagSev::Info, c.idx() as u64, self.boost[i] as u64, b"priority aging boost");
        }
        CorrectAction::Boost
    }
    /// 冷却结束后回落（老化提升不是永久的——长期观察后退回原优先级）。
    pub fn decay(&mut self, c: Class, now_ms: u64) {
        let i = c.idx();
        if let Some(t) = self.last_correct_ms[i] {
            // 冷却期的 10 倍时长无再修正 → 回落一级
            if now_ms.saturating_sub(t) >= AGING_COOLDOWN_MS * 10 && self.boost[i] > 0 {
                self.boost[i] -= 1;
                self.last_correct_ms[i] = Some(now_ms);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 4. 零样本观察窗说明
// ---------------------------------------------------------------------------

/// 零样本标注（主册「某类长期零样本 → 观察窗说明（不代表无风险）」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationNote {
    /// 有样本。
    HasSamples,
    /// 零样本但未超过观察窗（还不构成「长期」）。
    ZeroShort,
    /// 长期零样本：明确说明不代表无风险。
    ZeroLong,
}

impl ObservationNote {
    /// 判定：距上次样本时长 vs 观察窗。
    pub fn of(last_sample_ms: Option<u64>, now_ms: u64, window_ms: u64) -> Self {
        match last_sample_ms {
            None => ObservationNote::ZeroShort,
            Some(t) => {
                let gap = now_ms.saturating_sub(t);
                if gap < window_ms {
                    ObservationNote::HasSamples
                } else if gap < window_ms * 10 {
                    ObservationNote::ZeroShort
                } else {
                    ObservationNote::ZeroLong
                }
            }
        }
    }
    /// 呈现文案（不把「没数据」说成「没问题」）。
    pub fn text(&self) -> &'static str {
        match self {
            ObservationNote::HasSamples => "该类别有样本，曲线可解读",
            ObservationNote::ZeroShort => "该类别暂无样本（时间短，尚不构成结论）",
            ObservationNote::ZeroLong => "该类别长期零样本——无数据不等于无风险，观察窗内无调度发生",
        }
    }
    /// 是否需要在界面上显式提示（ZeroLong 必须提示）。
    pub fn needs_hint(&self) -> bool {
        matches!(self, ObservationNote::ZeroLong)
    }
}

// ---------------------------------------------------------------------------
// 5. energy 策略与大核可用性
// ---------------------------------------------------------------------------

/// energy 策略（主册「energy=balanced 策略（既有实测）在大核可用时保持」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnergyMode {
    /// 均衡（大核可用时的既有实测选择）。
    Balanced,
    /// 省电（大核不可用时的降级）。
    Powersave,
}

/// 大核可用性联动器。
#[derive(Clone, Copy, Debug)]
pub struct EnergyPolicy {
    pub big_core_available: bool,
    pub mode: EnergyMode,
    /// 因大核不可用而降级的次数。
    pub downgrades: u32,
}

impl EnergyPolicy {
    pub const fn new() -> Self {
        EnergyPolicy { big_core_available: true, mode: EnergyMode::Balanced, downgrades: 0 }
    }
    /// 大核可用性变化 → 调整策略（保持 balanced 优先）。
    pub fn on_core_change(&mut self, big_core_available: bool, sink: Option<&mut DiagSink>, now_ms: u64) {
        self.big_core_available = big_core_available;
        let want = if big_core_available { EnergyMode::Balanced } else { EnergyMode::Powersave };
        if want != self.mode {
            if !big_core_available {
                self.downgrades += 1;
                if let Some(s) = sink {
                    s.push("F047", 4, now_ms, DiagSev::Info, 0, 0, b"big core gone -> powersave");
                }
            }
            self.mode = want;
        }
    }
    /// 当前策略人话说明。
    pub fn text(&self) -> &'static str {
        match self.mode {
            EnergyMode::Balanced => "均衡策略（大核可用，既有实测口径）",
            EnergyMode::Powersave => "省电策略（大核不可用，已降级）",
        }
    }
}

// ---------------------------------------------------------------------------
// 6. 采样环内存预算自证
// ---------------------------------------------------------------------------

/// 采样环内存预算（主册「每类环形 10,000 条（内存 ~160KB）」）。
pub struct RingBudget;

impl RingBudget {
    /// 总字节 = 类别数 × 每类条数 × 单条字节。
    pub const fn total_bytes() -> usize {
        CLASSES * RING_PER_CLASS * SAMPLE_BYTES
    }
    /// 千字节（整数口径）。
    pub const fn total_kb() -> usize {
        Self::total_bytes() / 1_024
    }
    /// 是否在 160KB 预算内（主册是「约」——本件给出精确值与判定）。
    pub const fn within_160kb() -> bool {
        Self::total_bytes() <= 160 * 1_024
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F047-latbudget-ext");
    // 1) 预算表自洽（四类和 = 总预算 2000μs）。
    cs.add(
        "budget_sum_is_total",
        Budget::sum_matches_total() && Budget::of(Class::Input) == 500 && Budget::of(Class::Compose) == 800 && Budget::of(Class::Audio) == 300 && Budget::of(Class::Normal) == 400,
        "",
    );
    cs.add("budget_adr_note_present", Budget::adr_note().contains("ADR"), "");
    // 2) 优先级映射（输入最高带抢占 / 音频 deadline / 合成高 / 普通老化）。
    cs.add(
        "priority_map_matches_book",
        PriorityMap::of(Class::Input).preempt
            && PriorityMap::of(Class::Input).base == 0
            && PriorityMap::of(Class::Audio).deadline
            && PriorityMap::of(Class::Compose).base == 2
            && PriorityMap::of(Class::Normal).aging
            && PriorityMap::input_strictly_highest(),
        "",
    );
    cs.add("only_input_is_preemptive", PriorityMap::is_preemptive(Class::Input) && !PriorityMap::is_preemptive(Class::Audio), "");
    // 3) 有效优先级随老化提升而变高（数值变小），且不越过 0。
    cs.add(
        "effective_priority_boosted",
        PriorityMap::effective(Class::Normal, 0) == 3 && PriorityMap::effective(Class::Normal, 2) == 1 && PriorityMap::effective(Class::Input, 5) == 0,
        "",
    );
    // 4) 超标点快照只记超预算（不刷簿），且可下钻出幅度。
    let mut book = SnapshotBook::new();
    let over = ThreadSnapshot { tid: 7, prio: 0, class: Class::Input, at_ms: 1_000, latency_us: 750, over_budget: true };
    book.push(ThreadSnapshot { tid: 8, prio: 3, class: Class::Normal, at_ms: 1_000, latency_us: 100, over_budget: false });
    book.push(over);
    let mut out = [ThreadSnapshot { tid: 0, prio: 0, class: Class::Input, at_ms: 0, latency_us: 0, over_budget: false }; 4];
    let n = book.snapshot(&mut out);
    cs.add("snapshot_only_over_budget", n == 1 && out[0].tid == 7 && out[0].over_permille() == 500, "");
    // 5) 超预算幅度：未超标 = 0（不伪造幅度）。
    let inb = ThreadSnapshot { tid: 9, prio: 3, class: Class::Normal, at_ms: 0, latency_us: 200, over_budget: false };
    cs.add("over_permille_zero_when_in_budget", inb.over_permille() == 0, "");
    // 6) 老化自修正：归因到自身策略才动作；冷却生效；达上限放弃并报备。
    let mut sink = DiagSink::new();
    let mut sc = SelfCorrect::new();
    let a0 = sc.request(Class::Normal, false, 0, Some(&mut sink)); // 非自身原因 → 不动
    let a1 = sc.request(Class::Normal, true, 1_000, Some(&mut sink)); // 提一级
    let a2 = sc.request(Class::Normal, true, 2_000, Some(&mut sink)); // 冷却中 → 抑制
    cs.add(
        "aging_requires_self_cause_and_cooldown",
        a0 == CorrectAction::None && a1 == CorrectAction::Boost && a2 == CorrectAction::None && sc.cooled == 1 && sc.boost(Class::Normal) == 1,
        "",
    );
    // 冷却后继续提到上限 3，再提 → GiveUp + 报备
    let mut t = 1_000u64;
    for _ in 0..2 {
        t += AGING_COOLDOWN_MS;
        sc.request(Class::Normal, true, t, Some(&mut sink));
    }
    let give = sc.request(Class::Normal, true, t + AGING_COOLDOWN_MS, Some(&mut sink));
    cs.add(
        "aging_capped_then_giveup",
        sc.boost(Class::Normal) == AGING_MAX_BOOST && give == CorrectAction::GiveUp && sc.giveups == 1 && sink.count(DiagSev::Warn) == 1,
        "",
    );
    // 7) 老化提升会回落（不是永久改优先级）。
    let mut sc2 = SelfCorrect::new();
    sc2.request(Class::Normal, true, 0, None);
    let boosted = sc2.boost(Class::Normal);
    sc2.decay(Class::Normal, AGING_COOLDOWN_MS * 10);
    cs.add("aging_decays_back", boosted == 1 && sc2.boost(Class::Normal) == 0, "");
    // 8) 零样本观察窗：长期零样本必须显式提示（不把没数据说成没问题）。
    let s1 = ObservationNote::of(Some(0), 1_000, OBSERVE_WINDOW_MS);
    let s2 = ObservationNote::of(Some(0), OBSERVE_WINDOW_MS * 5, OBSERVE_WINDOW_MS);
    let s3 = ObservationNote::of(Some(0), OBSERVE_WINDOW_MS * 20, OBSERVE_WINDOW_MS);
    cs.add(
        "observation_note_three_states",
        s1 == ObservationNote::HasSamples && s2 == ObservationNote::ZeroShort && s3 == ObservationNote::ZeroLong && s3.needs_hint() && !s1.needs_hint(),
        "",
    );
    cs.add("zero_long_text_is_honest", ObservationNote::ZeroLong.text().contains("无数据不等于无风险"), "");
    // 9) energy 策略：大核可用保持 balanced，不可用降级并报备。
    let mut ep = EnergyPolicy::new();
    let mut sink2 = DiagSink::new();
    ep.on_core_change(false, Some(&mut sink2), 1_000);
    cs.add("energy_downgrade_reported", ep.mode == EnergyMode::Powersave && ep.downgrades == 1 && sink2.count(DiagSev::Info) == 1, "");
    ep.on_core_change(true, None, 2_000);
    cs.add("energy_restores_balanced", ep.mode == EnergyMode::Balanced, "");
    // 10) 采样环内存预算（4×10000×4 = 160000B ≈ 156KB ≤ 160KB）。
    cs.add(
        "ring_budget_160kb",
        RingBudget::total_bytes() == 160_000 && RingBudget::within_160kb() && RingBudget::total_kb() == 156,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_change_breaks_self_consistency_check() {
        // 自洽校验是常量级的：改了任何一类子预算而不同步总预算，
        // sum_matches_total 立刻为假（防止悄悄改一个数）
        assert!(Budget::sum_matches_total());
        assert_eq!(TOTAL_BUDGET_US, 500 + 800 + 300 + 400);
    }

    #[test]
    fn snapshot_book_covers_oldest_and_counts_total() {
        let mut b = SnapshotBook::new();
        for i in 0..40u32 {
            b.push(ThreadSnapshot { tid: i, prio: 0, class: Class::Input, at_ms: i as u64, latency_us: 900, over_budget: true });
        }
        assert_eq!(b.len(), 32);
        assert_eq!(b.total, 40, "被覆盖的不算不存在：总数照记");
        let mut out = [ThreadSnapshot { tid: 0, prio: 0, class: Class::Input, at_ms: 0, latency_us: 0, over_budget: false }; 32];
        b.snapshot(&mut out);
        assert_eq!(out[0].tid, 8, "最旧 8 条被覆盖");
    }

    #[test]
    fn aging_boost_never_exceeds_cap() {
        let mut sc = SelfCorrect::new();
        for i in 0..20u64 {
            sc.request(Class::Audio, true, i * AGING_COOLDOWN_MS, None);
        }
        assert_eq!(sc.boost(Class::Audio), AGING_MAX_BOOST);
        assert!(sc.giveups > 0);
    }

    #[test]
    fn observation_note_never_reports_no_risk() {
        // 「零样本」在任何状态下都不得给出「没问题」的结论
        for n in [ObservationNote::ZeroShort, ObservationNote::ZeroLong] {
            assert!(!n.text().contains("没问题"), "{:?}", n);
        }
    }

    #[test]
    fn energy_policy_text_is_stable() {
        let p = EnergyPolicy::new();
        assert_eq!(p.text(), "均衡策略（大核可用，既有实测口径）");
        let mut p2 = EnergyPolicy::new();
        p2.on_core_change(false, None, 0);
        assert_eq!(p2.text(), "省电策略（大核不可用，已降级）");
    }
}
