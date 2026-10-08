//! F052 堆碎片治理 · 深化件（AI-K1 深化批次三 · G-B-12）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「**每档空闲链用位图（O(1) 分配）**」 | [`BitAlloc`] 位图分配器（首个零位 O(1) 定位，分配/释放均 O(1)） |
//! | 2 | 【设计细节】「**大块直通阈值 512B**」 | [`ClassTable`] 六档 + 直通分界（一处一事实，档边界与直通阈值同源） |
//! | 3 | 【状态与异常】「**某档耗尽 → 相邻档切分（带切分开销标注）**」 | [`SplitPolicy`] 相邻档切分（大档切小档，开销必须记进账） |
//! | 4 | 【状态与异常】「**分配失败路径全测（panic 演练 B-2903 场景含）**」 | [`FailPath`] 分配失败路径（全档失败 → 直通失败 → 切分失败 → 演练记录） |
//! | 5 | 【状态与异常】「**碎片率 >25% → 告警 + 归因**」 | [`FragAlarm`] 告警状态机（迟滞解除 + 归因输出，与主域 AllocSpectrum 对接） |
//! | 6 | 【验收判据】「**六档分配延迟 P99 <1μs**」 | [`AllocLatency`] 分档延迟账（桶上界口径，六档各自达标） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::{DiagSev, DiagSink};

// ---------------------------------------------------------------------------
// 常量（主册【功能定义】【设计细节】）
// ---------------------------------------------------------------------------

/// 六档尺寸（字节）：8/16/32/64/128/256。
pub const CLASS_BYTES: [u32; 6] = [8, 16, 32, 64, 128, 256];
/// 大块直通阈值 512B（主册【设计细节】）：≥512B 走直通。
pub const PASSTHROUGH_THRESHOLD: u32 = 512;
/// 每档槽位上限（位图宽度）。
pub const SLOTS_PER_CLASS: usize = 512;
/// 位图字数（512 位 / 64）。
pub const BITMAP_WORDS: usize = SLOTS_PER_CLASS / 64;
/// 碎片率告警线 25%（千分 250）。
pub const FRAG_ALARM_PERMILLE: u32 = 250;
/// 碎片率达标线 15%（千分 150，主册【功能定义】「长期运行碎片率 <15%」）。
pub const FRAG_TARGET_PERMILLE: u32 = 150;
/// 告警迟滞（解除需低于告警线的这个比例）。
pub const ALARM_HYSTERESIS_PERMILLE: u32 = 20;
/// 分配延迟 P99 红线 1μs（主册【验收判据】）。
pub const LATENCY_REDLINE_NS: u32 = 1_000;
/// 切分开销（纳秒）：大档切小档需要拆块与重链，记入账本。
pub const SPLIT_COST_NS: u32 = 120;

// ---------------------------------------------------------------------------
// 1. 位图分配器（O(1) 分配）
// ---------------------------------------------------------------------------

/// 位图分配器：一档的空闲链用位图表达，分配 = 找首个零位并置一。
///
/// 主册「每档空闲链用位图（O(1) 分配）」——O(1) 的前提是**用位运算找零位**，
/// 而不是遍历；本件按 64 位字扫描（字数固定 8），最坏 8 次比较，与槽数无关。
#[derive(Clone, Copy, Debug)]
pub struct BitAlloc {
    /// 占用位图（1 = 已占用）。
    bits: [u64; BITMAP_WORDS],
    /// 已占用槽数。
    pub used: u32,
    /// 累计分配次数。
    pub allocs: u64,
    /// 累计释放次数。
    pub frees: u64,
    /// 分配失败次数（位图满）。
    pub failures: u64,
}

impl BitAlloc {
    pub const fn new() -> Self {
        BitAlloc { bits: [0u64; BITMAP_WORDS], used: 0, allocs: 0, frees: 0, failures: 0 }
    }
    /// 分配一个槽（返回槽号）。O(1)：按字扫描首个非零「空闲位」。
    pub fn alloc(&mut self) -> Option<u32> {
        for w in 0..BITMAP_WORDS {
            let inv = !self.bits[w];
            if inv != 0 {
                let bit = inv.trailing_zeros() as u32;
                self.bits[w] |= 1u64 << bit;
                self.used += 1;
                self.allocs += 1;
                return Some((w as u32) * 64 + bit);
            }
        }
        self.failures += 1;
        None
    }
    /// 释放一个槽（O(1)：直接清位）。重复释放（清已清的位）视为错误并计数。
    pub fn free(&mut self, slot: u32) -> bool {
        let w = (slot / 64) as usize;
        if w >= BITMAP_WORDS {
            return false;
        }
        let bit = slot % 64;
        let mask = 1u64 << bit;
        if self.bits[w] & mask == 0 {
            self.failures += 1; // 重复释放：记入失败计数，不静默
            return false;
        }
        self.bits[w] &= !mask;
        self.used = self.used.saturating_sub(1);
        self.frees += 1;
        true
    }
    /// 是否满。
    pub fn is_full(&self) -> bool {
        self.used >= SLOTS_PER_CLASS as u32
    }
    /// 碎片率千分（已占用 / 总槽——本档视角的「用得多碎」）。
    pub fn occupancy_permille(&self) -> u32 {
        ((self.used as u64 * 1000) / SLOTS_PER_CLASS as u64) as u32
    }
    /// 分配/释放是否一一配对（契约自检：差值应等于当前占用）。
    pub fn pairing_consistent(&self) -> bool {
        self.allocs.saturating_sub(self.frees) == self.used as u64
    }
}

// ---------------------------------------------------------------------------
// 2. 档位表与直通分界
// ---------------------------------------------------------------------------

/// 档位判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassFit {
    /// 落在某一档（给出档号）。
    Class(usize),
    /// 大块直通（≥512B）。
    Passthrough,
}

/// 档位表（六档 + 直通，一处一事实）。
pub struct ClassTable;

impl ClassTable {
    /// 尺寸 → 档位或直通（主册：六档 + 大块直通阈值 512B）。
    pub fn fit(bytes: u32) -> ClassFit {
        for (i, &c) in CLASS_BYTES.iter().enumerate() {
            if bytes <= c {
                return ClassFit::Class(i);
            }
        }
        if bytes < PASSTHROUGH_THRESHOLD {
            // 256 < bytes < 512：主册口径下直通阈值为 512，这段落最大档
            return ClassFit::Class(CLASS_BYTES.len() - 1);
        }
        ClassFit::Passthrough
    }
    /// 档位字节数（越界返回 0——不 panic）。
    pub fn class_bytes(i: usize) -> u32 {
        if i < CLASS_BYTES.len() {
            CLASS_BYTES[i]
        } else {
            0
        }
    }
    /// 直通阈值之上的分配不进分级池（不污染档位统计）。
    pub const fn passthrough_threshold() -> u32 {
        PASSTHROUGH_THRESHOLD
    }
    /// 内部碎片字节：按档分配时请求尺寸与档尺寸的差（直通为 0）。
    pub fn internal_fragment_bytes(bytes: u32) -> u32 {
        match Self::fit(bytes) {
            ClassFit::Class(i) => Self::class_bytes(i).saturating_sub(bytes),
            ClassFit::Passthrough => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 3. 相邻档切分（带开销标注）
// ---------------------------------------------------------------------------

/// 切分裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitPlan {
    /// 本档有空闲，无需切分。
    Direct,
    /// 从上一档（更大的档）切一块下来。
    SplitFrom {
        /// 源档号。
        from: usize,
        /// 可切出的块数。
        pieces: u32,
        /// 切分开销（纳秒）——必须记进账。
        cost_ns: u32,
    },
    /// 无更大档可切（全耗尽）。
    Exhausted,
}

/// 切分策略（主册「某档耗尽 → 相邻档切分（带切分开销标注）」）。
#[derive(Clone, Copy, Debug)]
pub struct SplitPolicy {
    /// 切分次数。
    pub splits: u64,
    /// 累计切分开销（纳秒）。
    pub cost_ns: u64,
    /// 无档可切的次数。
    pub exhausted: u64,
}

impl SplitPolicy {
    pub const fn new() -> Self {
        SplitPolicy { splits: 0, cost_ns: 0, exhausted: 0 }
    }
    /// 裁定：目标档满时，从更大的相邻档切。
    ///
    /// 相邻 = 向上找最近的更大档（主册「相邻档切分」）；跨多档跳切会加剧碎片，
    /// 故只取最近一档。
    pub fn plan(&self, target: usize, class_free: &[bool; 6]) -> SplitPlan {
        if target < 6 && class_free[target] {
            return SplitPlan::Direct;
        }
        let mut i = target + 1;
        while i < 6 {
            if class_free[i] {
                // 大档切成小档：块数 = 大档尺寸 / 目标档尺寸
                let pieces = CLASS_BYTES[i] / CLASS_BYTES[target.min(5)];
                return SplitPlan::SplitFrom { from: i, pieces, cost_ns: SPLIT_COST_NS };
            }
            i += 1;
        }
        SplitPlan::Exhausted
    }
    /// 执行一次切分（记账）。
    pub fn apply(&mut self, plan: SplitPlan) -> bool {
        match plan {
            SplitPlan::Direct => true,
            SplitPlan::SplitFrom { cost_ns, .. } => {
                self.splits += 1;
                self.cost_ns += cost_ns as u64;
                true
            }
            SplitPlan::Exhausted => {
                self.exhausted += 1;
                false
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 4. 分配失败路径（panic 演练 B-2903）
// ---------------------------------------------------------------------------

/// 失败阶段（主册「分配失败路径全测」——每一段都要有归宿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailStage {
    /// 本档分配成功。
    None,
    /// 本档耗尽。
    ClassExhausted,
    /// 相邻档切分失败。
    SplitFailed,
    /// 直通也失败（内存真的不够）。
    PassthroughFailed,
}

/// 分配失败路径演练账（B-2903 panic 演练场景含）。
#[derive(Clone, Copy, Debug)]
pub struct FailPath {
    /// 各阶段被触发次数。
    pub hits: [u32; 4],
    /// 演练次数（注入全失败场景）。
    pub drills: u32,
    /// 演练中成功走到「可解释失败」的次数（判据：失败必须可解释，不是崩溃）。
    pub explained: u32,
}

impl FailPath {
    pub const fn new() -> Self {
        FailPath { hits: [0; 4], drills: 0, explained: 0 }
    }
    fn idx(s: FailStage) -> usize {
        match s {
            FailStage::None => 0,
            FailStage::ClassExhausted => 1,
            FailStage::SplitFailed => 2,
            FailStage::PassthroughFailed => 3,
        }
    }
    /// 记录一次分配结果。
    pub fn note(&mut self, s: FailStage) {
        self.hits[FailPath::idx(s)] += 1;
    }
    /// 演练一次「全路径失败」：返回是否给出了可解释的失败（而不是 panic）。
    pub fn drill(&mut self, alloc_ok: bool, split_ok: bool, passthrough_ok: bool) -> FailStage {
        self.drills += 1;
        if alloc_ok {
            self.note(FailStage::None);
            return FailStage::None;
        }
        self.note(FailStage::ClassExhausted);
        if split_ok {
            return FailStage::ClassExhausted;
        }
        self.note(FailStage::SplitFailed);
        if passthrough_ok {
            return FailStage::SplitFailed;
        }
        self.note(FailStage::PassthroughFailed);
        // 走到这一步仍然是「可解释的失败」——给出阶段而不是崩
        self.explained += 1;
        FailStage::PassthroughFailed
    }
    /// 失败阶段文案（用户/开发者看得懂，不裸抛错误码）。
    pub fn text(s: FailStage) -> &'static str {
        match s {
            FailStage::None => "分配成功",
            FailStage::ClassExhausted => "该尺寸档已用尽，正从相邻档切分",
            FailStage::SplitFailed => "相邻档也无空闲，直通区尝试中",
            FailStage::PassthroughFailed => "内核堆已耗尽：分配失败（已记录，未崩溃）",
        }
    }
}

// ---------------------------------------------------------------------------
// 5. 碎片率告警（>25% 告警 + 归因，迟滞解除）
// ---------------------------------------------------------------------------

/// 归因模式（与主域 `AllocSpectrum` 四模式同名同义，本件只做告警面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FragCause {
    /// 小对象风暴。
    SmallObjectStorm,
    /// 直通区churn。
    PassthroughChurn,
    /// 档边界 churn。
    ClassEdgeChurn,
    /// 混合谱。
    MixedSpectrum,
    /// 数据不足（不出归因——数据不足不拍脑袋）。
    InsufficientData,
}

impl FragCause {
    pub fn text(&self) -> &'static str {
        match self {
            FragCause::SmallObjectStorm => "小对象分配风暴（大量短命小对象）",
            FragCause::PassthroughChurn => "直通区反复分配释放（大块抖动）",
            FragCause::ClassEdgeChurn => "档边界尺寸抖动（尺寸跨档反复）",
            FragCause::MixedSpectrum => "多种分配模式混合（无单一主因）",
            FragCause::InsufficientData => "样本不足，暂不归因",
        }
    }
}

/// 碎片率告警状态机（迟滞解除，防边界抖动刷告警）。
#[derive(Clone, Copy, Debug)]
pub struct FragAlarm {
    pub active: bool,
    /// 告警次数。
    pub raised: u32,
    /// 最近一次告警的碎片率千分。
    pub last_permille: u32,
    /// 最近一次归因。
    pub last_cause: FragCause,
}

impl FragAlarm {
    pub const fn new() -> Self {
        FragAlarm { active: false, raised: 0, last_permille: 0, last_cause: FragCause::InsufficientData }
    }
    /// 上报一次碎片率与归因（`samples` 用于判定数据是否充足）。
    pub fn report(&mut self, permille: u32, cause: FragCause, samples: u32, sink: Option<&mut DiagSink>, now_ms: u64) -> bool {
        self.last_permille = permille;
        // 数据不足时保留主域归因，但告警面标注「数据不足」
        let effective_cause = if samples < 1_000 { FragCause::InsufficientData } else { cause };
        self.last_cause = effective_cause;
        if !self.active && permille > FRAG_ALARM_PERMILLE {
            self.active = true;
            self.raised += 1;
            if let Some(s) = sink {
                s.push("F052", 1, now_ms, DiagSev::Warn, permille as u64, effective_cause as u64, b"frag over 25%");
            }
            return true;
        }
        // 迟滞解除：需低于告警线 20‰（=230‰）才解除，防抖动
        if self.active && permille + ALARM_HYSTERESIS_PERMILLE < FRAG_ALARM_PERMILLE {
            self.active = false;
        }
        false
    }
    /// 当前是否达标（<15%）。
    pub fn within_target(&self, permille: u32) -> bool {
        permille < FRAG_TARGET_PERMILLE
    }
}

// ---------------------------------------------------------------------------
// 6. 六档分配延迟账（P99 <1μs）
// ---------------------------------------------------------------------------

/// 延迟桶（对数：50ns 起 1.25 倍递增，24 桶覆盖到 ~7μs 以上）。
pub const LAT_BUCKETS: usize = 24;

fn lat_bucket(ns: u32) -> usize {
    let mut b = 0usize;
    let mut lo = 50u32;
    while b + 1 < LAT_BUCKETS && ns > lo {
        lo = lo + lo / 4;
        b += 1;
    }
    b
}

/// 六档分配延迟账（主册「六档分配延迟 P99 <1μs」）。
#[derive(Clone, Copy, Debug)]
pub struct AllocLatency {
    counts: [[u32; LAT_BUCKETS]; 6],
    total: [u32; 6],
}

impl AllocLatency {
    pub const fn new() -> Self {
        AllocLatency { counts: [[0; LAT_BUCKETS]; 6], total: [0; 6] }
    }
    pub fn note(&mut self, class: usize, ns: u32) {
        if class >= 6 {
            return;
        }
        self.counts[class][lat_bucket(ns)] += 1;
        self.total[class] += 1;
    }
    /// 某档 P99 上界（桶口径）。
    pub fn p99(&self, class: usize) -> Option<u32> {
        if class >= 6 || self.total[class] == 0 {
            return None;
        }
        let target = ((self.total[class] as u64 * 99 + 99) / 100) as u32;
        let mut acc = 0u32;
        let mut lo = 50u32;
        for b in 0..LAT_BUCKETS {
            acc += self.counts[class][b];
            if acc >= target {
                return Some(lo);
            }
            lo = lo + lo / 4;
        }
        Some(lo)
    }
    /// 六档全部 <1μs（任一档无样本 = 不成立）。
    pub fn all_classes_pass(&self) -> bool {
        (0..6).all(|c| matches!(self.p99(c), Some(v) if v < LATENCY_REDLINE_NS))
    }
    /// 未跑过样本的档位数（诚实暴露覆盖率缺口）。
    pub fn classes_without_samples(&self) -> usize {
        (0..6).filter(|c| self.total[*c] == 0).count()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F052-heapfrag-ext");
    // 1) 位图分配 O(1)：分配拿首个零位，释放清位，重复释放被记。
    let mut b = BitAlloc::new();
    let s0 = b.alloc();
    let s1 = b.alloc();
    cs.add("bitmap_alloc_first_free", s0 == Some(0) && s1 == Some(1) && b.used == 2, "");
    let freed = b.free(0);
    let double = b.free(0);
    cs.add("bitmap_free_and_double_free_detected", freed && !double && b.used == 1 && b.failures == 1, "");
    // 越界释放不得 panic 也不得误清位
    cs.add("bitmap_free_out_of_range", !b.free(999_999), "");
    // 2) 分配/释放配对契约（差值 = 占用）。
    cs.add("bitmap_pairing_consistent", b.pairing_consistent(), "");
    // 3) 位图填满后分配失败（不越界、不panic）。
    let mut full = BitAlloc::new();
    let mut ok = 0u32;
    while full.alloc().is_some() {
        ok += 1;
    }
    cs.add("bitmap_full_fails_cleanly", ok == SLOTS_PER_CLASS as u32 && full.is_full() && full.alloc().is_none(), "");
    // 4) 档位表：六档 + 512B 直通；256<size<512 落最大档。
    cs.add(
        "class_table_fit",
        ClassTable::fit(8) == ClassFit::Class(0)
            && ClassTable::fit(9) == ClassFit::Class(1)
            && ClassTable::fit(300) == ClassFit::Class(5)
            && ClassTable::fit(512) == ClassFit::Passthrough
            && ClassTable::fit(4_096) == ClassFit::Passthrough,
        "",
    );
    cs.add("class_table_bytes_safe", ClassTable::class_bytes(0) == 8 && ClassTable::class_bytes(99) == 0, "");
    // 5) 内部碎片（按档分配时档尺寸 - 请求尺寸；直通为 0）。
    cs.add(
        "internal_fragment_computed",
        ClassTable::internal_fragment_bytes(8) == 0 && ClassTable::internal_fragment_bytes(9) == 7 && ClassTable::internal_fragment_bytes(4_096) == 0,
        "",
    );
    // 6) 相邻档切分：只取最近更大档，带开销。
    let mut sp = SplitPolicy::new();
    let free = [false, false, true, false, false, false]; // 仅档 2 有空闲
    let p1 = sp.plan(0, &free); // 档 0 满 → 切档 2（不是档 5）
    cs.add("split_from_nearest_bigger", matches!(p1, SplitPlan::SplitFrom { from: 2, .. }) && sp.apply(p1), "");
    // 无更大档可切 → Exhausted
    let p2 = sp.plan(4, &[false; 6]);
    cs.add("split_exhausted", p2 == SplitPlan::Exhausted && !sp.apply(p2) && sp.exhausted == 1, "");
    // 本档有空闲 → 直接分配，无切分开销
    cs.add("split_direct", sp.plan(0, &[true, false, false, false, false, false]) == SplitPlan::Direct, "");
    cs.add("split_cost_recorded", sp.splits == 1 && sp.cost_ns == SPLIT_COST_NS as u64, "");
    // 7) 分配失败路径：四阶段各自可解释（不崩）。
    let mut fp = FailPath::new();
    let s = fp.drill(false, false, false);
    cs.add(
        "fail_path_explained",
        s == FailStage::PassthroughFailed
            && fp.hits[1] == 1
            && fp.hits[2] == 1
            && fp.hits[3] == 1
            && fp.explained == 1
            && FailPath::text(s) == "内核堆已耗尽：分配失败（已记录，未崩溃）",
        "",
    );
    // 中途成功不算走到失败终态
    let s2 = fp.drill(false, true, false);
    cs.add("fail_path_mid_rescue", s2 == FailStage::ClassExhausted && fp.explained == 1, "");
    // 8) 碎片率告警：>25% 触发 + 诊断报备 + 迟滞解除。
    let mut sink = DiagSink::new();
    let mut fa = FragAlarm::new();
    let raised = fa.report(300, FragCause::SmallObjectStorm, 5_000, Some(&mut sink), 1_000);
    let still = fa.report(260, FragCause::SmallObjectStorm, 5_000, Some(&mut sink), 2_000); // 仍 >250
    fa.report(200, FragCause::MixedSpectrum, 5_000, Some(&mut sink), 3_000); // <230 → 解除
    cs.add(
        "frag_alarm_with_hysteresis",
        raised && !still && !fa.active && fa.raised == 1 && sink.count(DiagSev::Warn) == 1 && fa.last_cause == FragCause::MixedSpectrum,
        "",
    );
    // 边界抖动：250~260 之间反复不重复告警
    let mut fa2 = FragAlarm::new();
    fa2.report(260, FragCause::MixedSpectrum, 5_000, None, 0);
    for i in 0..5 {
        fa2.report(255, FragCause::MixedSpectrum, 5_000, None, i);
    }
    cs.add("frag_alarm_no_flap", fa2.raised == 1, "");
    // 9) 数据不足不出归因（不拍脑袋）。
    let mut fa3 = FragAlarm::new();
    fa3.report(900, FragCause::SmallObjectStorm, 10, None, 0);
    cs.add("frag_insufficient_data", fa3.last_cause == FragCause::InsufficientData, "");
    // 10) 达标线 15% 判定。
    cs.add("frag_target_15pct", fa3.within_target(149) && !fa3.within_target(150), "");
    // 11) 六档延迟 P99 <1μs。
    let mut al = AllocLatency::new();
    for c in 0..6 {
        for _ in 0..100 {
            al.note(c, 300);
        }
    }
    cs.add("alloc_latency_all_classes_pass", al.all_classes_pass() && al.classes_without_samples() == 0, "");
    // 缺一档样本就不算全绿（覆盖率缺口不隐藏）
    let mut al2 = AllocLatency::new();
    for c in 0..5 {
        for _ in 0..100 {
            al2.note(c, 300);
        }
    }
    cs.add("alloc_latency_gap_exposed", !al2.all_classes_pass() && al2.classes_without_samples() == 1, "");
    // 慢档不达标（不粉饰）
    let mut al3 = AllocLatency::new();
    for c in 0..6 {
        for _ in 0..100 {
            al3.note(c, if c == 3 { 4_000 } else { 300 });
        }
    }
    cs.add("alloc_latency_slow_class_fails", !al3.all_classes_pass(), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_alloc_is_o1_regardless_of_fill_level() {
        // 填到 511 个槽后再分配一次：定位开销与填充度无关（仍是按字扫描）
        let mut b = BitAlloc::new();
        for _ in 0..511 {
            assert!(b.alloc().is_some());
        }
        let s = b.alloc();
        assert_eq!(s, Some(511));
        assert!(b.is_full());
    }

    #[test]
    fn pairing_contract_holds_after_mixed_operations() {
        let mut b = BitAlloc::new();
        let a = b.alloc().unwrap();
        let c = b.alloc().unwrap();
        b.free(a);
        let d = b.alloc().unwrap();
        assert!(b.pairing_consistent());
        assert_eq!(b.used, 2);
        let _ = (c, d);
    }

    #[test]
    fn passthrough_never_enters_class_pools() {
        // 直通阈值之上不占分级池槽位
        assert_eq!(ClassTable::fit(512), ClassFit::Passthrough);
        assert_eq!(ClassTable::passthrough_threshold(), 512);
    }

    #[test]
    fn split_policy_prefers_nearest_bigger_class() {
        let sp = SplitPolicy::new();
        let free = [false, false, false, false, true, true]; // 档 4、5 空闲
        assert!(matches!(sp.plan(3, &free), SplitPlan::SplitFrom { from: 4, .. }), "取最近的档 4，不是档 5");
    }

    #[test]
    fn fail_path_never_panics_on_total_exhaustion() {
        let mut f = FailPath::new();
        for _ in 0..10 {
            assert_eq!(f.drill(false, false, false), FailStage::PassthroughFailed);
        }
        assert_eq!(f.explained, 10);
        assert!(FailPath::text(FailStage::PassthroughFailed).contains("已记录，未崩溃"));
    }

    #[test]
    fn latency_buckets_are_monotonic_and_bounded() {
        let mut last = 0usize;
        for ns in [0u32, 50, 100, 500, 1_000, 5_000, 50_000, u32::MAX] {
            let b = lat_bucket(ns);
            assert!(b >= last);
            last = b;
        }
        assert!(last < LAT_BUCKETS);
    }
}
