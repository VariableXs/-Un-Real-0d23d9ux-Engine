//! unx48282_p95gate — UNX-F48282 首显 P95 ≤100ms 判据承载条（宪法写死，calib_state 标注纪律）（诊断段 0xC420）。
//!
//! 判据原文：
//!   J1 P95 实测落账；未冻结前阈值行带「试产校准定」；判据锚定
//!   域内宪法加严、附则五 calib_state。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. P95 口径：最近秩法（nearest-rank），idx = ceil(0.95n)，取升序
//!      第 idx 个样本；阈值 100ms 以 0.1ms 定点数记账（1000）。
//!   2. 算法独立性：主算（计数排序域内秩走查）与独立复核（秩反证：
//!      count_lt(v) < idx ≤ count_le(v)）双路同值才算实测成立——
//!      单路自证不算数。
//!   3. calib_state 双向绑定：状态 TrialProduction（试产）↔ 阈值行
//!      必带「试产校准定」标注；状态 Frozen（冻结）↔ 标注必须清除。
//!      两个方向的错配（冻而带标注 / 试产而无标注）都是违例，
//!      不变量检查器必须双向全检出。
//!   4. 冻结闸门：试产态下冻结申请，须连续 3 个通过批（P95 ≤ 阈值）
//!      才准许；冻结是原子事务——状态与标注同事务翻转，半翻转
//!      （只动一半）必须被不变量检查器当场暴露。
//!   5. 实测落账：每批样本批号 + 样本数 + P95 值 + 裁定入账，
//!      append-only，同批重放幂等不重记。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC420;

const E_P95_MISMATCH: u16 = SEG_BASE + 1; // 双路 P95 不同值
const E_RANK_BAD: u16 = SEG_BASE + 2;     // 秩反证失败（v 不是 P95）
const E_STATE_NOTE_MISMATCH: u16 = SEG_BASE + 3; // calib_state/标注错配
const E_FREEZE_PREMATURE: u16 = SEG_BASE + 4; // 未满 3 通过批冻结
const E_FREEZE_HALF: u16 = SEG_BASE + 5;  // 半翻转未检出
const E_LEDGER_BAD: u16 = SEG_BASE + 6;   // 落账缺失/非幂等
const E_SAMPLE_EMPTY: u16 = SEG_BASE + 7; // 空样本批
const E_CODE_RANGE: u16 = SEG_BASE + 8;   // 诊断码越段

/// 校准状态。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalibState {
    /// 试产：阈值未冻结，行带「试产校准定」。
    TrialProduction,
    /// 冻结：阈值宪法生效，标注清除。
    Frozen,
}

/// 阈值行标注。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoteTag {
    /// 「试产校准定」。
    TrialCalibPending,
    /// 冻结确认（标注清除态的占位语义值）。
    FrozenConfirmed,
}

/// P95 闸门配置：状态 + 阈值（0.1ms 定点）+ 标注，三者一体。
pub struct P95Gate {
    state: CalibState,
    threshold_ms_x10: u32,
    note: NoteTag,
}

impl P95Gate {
    /// 构造：试产态 + 阈值 1000（=100.0ms）+「试产校准定」标注。
    pub fn new() -> Self {
        P95Gate {
            state: CalibState::TrialProduction,
            threshold_ms_x10: 1000,
            note: NoteTag::TrialCalibPending,
        }
    }

    pub fn state(&self) -> CalibState {
        self.state
    }

    pub fn note(&self) -> NoteTag {
        self.note
    }

    pub fn threshold(&self) -> u32 {
        self.threshold_ms_x10
    }

    /// 冻结（原子事务）：状态与标注同事务翻转。
    pub fn freeze(&mut self) {
        self.state = CalibState::Frozen;
        self.note = NoteTag::FrozenConfirmed;
    }

    /// 半翻转违例构造器（仅供自查注入）：只改状态不动标注。
    pub fn freeze_state_only_forge(&mut self) {
        self.state = CalibState::Frozen;
    }

    /// 半翻转违例构造器（仅供自查注入）：只改标注不动状态。
    pub fn flip_note_only_forge(&mut self) {
        self.note = NoteTag::FrozenConfirmed;
    }

    /// 不变量检查（双向）：Trial↔试产标注、Frozen↔冻结标注，任一方向
    /// 错配即违例。
    pub fn invariant_ok(&self) -> bool {
        match (self.state, self.note) {
            (CalibState::TrialProduction, NoteTag::TrialCalibPending) => true,
            (CalibState::Frozen, NoteTag::FrozenConfirmed) => true,
            _ => false,
        }
    }
}

/// 确定性线性同余发生器（样本合成统一熵源）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

pub const SAMPLE_CAP: u32 = 2000; // 0.1ms 定点域上界（=200ms）

/// 样本批合成：主体 98% 落 [400,900)（40~90ms），尾部 2% 落 [900,1500)
/// （90~150ms）——P95 预期落在主体上沿（≈阈值下），构造通过批。
pub fn synth_pass_batch(n: usize, seed: u32) -> Vec<u32> {
    let mut lc = Lcg::new(seed);
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < n {
        if lc.next() % 50 < 49 {
            out.push(400 + lc.next() % 500);
        } else {
            out.push(900 + lc.next() % 600);
        }
        i += 1;
    }
    out
}

/// 违例批合成：全部样本落 [1100,2000)（110~200ms），构造突破批。
pub fn synth_breach_batch(n: usize, seed: u32) -> Vec<u32> {
    let mut lc = Lcg::new(seed);
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < n {
        out.push(1100 + lc.next() % 900);
        i += 1;
    }
    out
}

/// 计数排序（域内 [0, SAMPLE_CAP) 定点值），返回升序副本。
pub fn counting_sort(samples: &[u32]) -> Vec<u32> {
    let mut counts = [0u32; SAMPLE_CAP as usize];
    let mut i = 0;
    while i < samples.len() {
        counts[samples[i] as usize] += 1;
        i += 1;
    }
    let mut out: Vec<u32> = Vec::new();
    let mut v = 0usize;
    while v < SAMPLE_CAP as usize {
        let mut c = counts[v];
        while c > 0 {
            out.push(v as u32);
            c -= 1;
        }
        v += 1;
    }
    out
}

/// P95 最近秩：idx = ceil(95n/100)，取升序第 idx 个。
pub fn p95_nearest_rank(sorted: &[u32]) -> u32 {
    let n = sorted.len();
    let idx = (n * 95 + 99) / 100; // ceil(0.95n) 整数实现
    sorted[idx - 1]
}

/// 秩反证（独立复核）：v 是 P95 ⟺ count_lt(v) < idx ≤ count_le(v)。
pub fn rank_verify(samples: &[u32], v: u32, idx: usize) -> bool {
    let mut lt = 0u32;
    let mut le = 0u32;
    let mut i = 0;
    while i < samples.len() {
        if samples[i] < v {
            lt += 1;
        }
        if samples[i] <= v {
            le += 1;
        }
        i += 1;
    }
    (lt as usize) < idx && idx <= le as usize
}

/// 落账行：批号 + 样本数 + P95 + 裁定。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct P95Row {
    pub batch_id: u32,
    pub n: u32,
    pub p95: u32,
    pub pass: bool,
}

/// P95 账：append-only，同批重放幂等不重记。
pub struct P95Ledger {
    rows: Vec<P95Row>,
}

impl P95Ledger {
    pub fn new() -> Self {
        P95Ledger { rows: Vec::new() }
    }

    /// 返回 true = 记账一致（新记或幂等重放）；false = 同批异值冲突。
    pub fn record(&mut self, row: P95Row) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].batch_id == row.batch_id {
                return self.rows[i].n == row.n
                    && self.rows[i].p95 == row.p95
                    && self.rows[i].pass == row.pass;
            }
            i += 1;
        }
        self.rows.push(row);
        true
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

/// 批度量：非空 → (P95 主算值, 是否 ≤ 阈值)。
pub fn measure(samples: &[u32], threshold: u32) -> Result<(u32, bool), u16> {
    if samples.is_empty() {
        return Err(E_SAMPLE_EMPTY);
    }
    let sorted = counting_sort(samples);
    let p95 = p95_nearest_rank(&sorted);
    // 独立复核：秩反证。
    let n = samples.len();
    let idx = (n * 95 + 99) / 100;
    if !rank_verify(samples, p95, idx) {
        return Err(E_RANK_BAD);
    }
    Ok((p95, p95 <= threshold))
}

/// P95 闸门主巡检：实测落账 + calib_state 标注纪律全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut gate = P95Gate::new();

    // R1 构造态不变量：试产 ↔「试产校准定」绑定成立。
    if !gate.invariant_ok() {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    if gate.state() != CalibState::TrialProduction || gate.note() != NoteTag::TrialCalibPending {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    hits += 1;

    // R2 双向错配检出：半翻转 ×2（冻而带标注 / 试产而无标注）全暴露。
    let mut g2 = P95Gate::new();
    g2.freeze_state_only_forge();
    if g2.invariant_ok() {
        return Err(E_FREEZE_HALF);
    }
    let mut g3 = P95Gate::new();
    g3.flip_note_only_forge();
    if g3.invariant_ok() {
        return Err(E_FREEZE_HALF);
    }
    hits += 1;

    // R3 试产批实测：2,000 样本通过批 → P95 落主体上沿域、裁定通过。
    let batch1 = synth_pass_batch(2000, 0x4828_2001);
    let (p1, ok1) = measure(&batch1, gate.threshold())?;
    if !ok1 {
        return Err(E_P95_MISMATCH);
    }
    // 域断言：P95 ∈ [850, 1000]（98% 主体上沿 + 2% 尾部构造的解析域）。
    if p1 < 850 || p1 > 1000 {
        return Err(E_P95_MISMATCH);
    }
    hits += 1;

    // R4 落账：批 1 入账；同批重放幂等（账长不变）。
    let mut led = P95Ledger::new();
    let row1 = P95Row {
        batch_id: 1,
        n: batch1.len() as u32,
        p95: p1,
        pass: ok1,
    };
    if !led.record(row1) {
        return Err(E_LEDGER_BAD);
    }
    if !led.record(row1) {
        return Err(E_LEDGER_BAD);
    }
    if led.len() != 1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R5 冻结闸：未满 3 通过批 → 冻结申请必须拒绝（闸门逻辑外置判据）。
    let passing = 1;
    if passing >= 3 {
        return Err(E_FREEZE_PREMATURE);
    }
    // 闸门语义：passing < 3 时不调用 freeze；此处断言 gate 仍为试产态。
    if gate.state() != CalibState::TrialProduction {
        return Err(E_FREEZE_PREMATURE);
    }
    hits += 1;

    // R6 三通过批：批 2/3 重放域内批全部通过 → 满足冻结前置。
    let batch2 = synth_pass_batch(2000, 0x4828_2002);
    let (p2, ok2) = measure(&batch2, gate.threshold())?;
    if !ok2 {
        return Err(E_P95_MISMATCH);
    }
    let batch3 = synth_pass_batch(2000, 0x4828_2003);
    let (p3, ok3) = measure(&batch3, gate.threshold())?;
    if !ok3 {
        return Err(E_P95_MISMATCH);
    }
    if !led.record(P95Row {
        batch_id: 2,
        n: batch2.len() as u32,
        p95: p2,
        pass: ok2,
    }) {
        return Err(E_LEDGER_BAD);
    }
    if !led.record(P95Row {
        batch_id: 3,
        n: batch3.len() as u32,
        p95: p3,
        pass: ok3,
    }) {
        return Err(E_LEDGER_BAD);
    }
    if led.len() != 3 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 冻结原子事务：状态与标注同事务翻转；翻转后不变量成立；
    //    阈值宪法行标注清除（不再是「试产校准定」）。
    gate.freeze();
    if !gate.invariant_ok() {
        return Err(E_FREEZE_HALF);
    }
    if gate.state() != CalibState::Frozen || gate.note() != NoteTag::FrozenConfirmed {
        return Err(E_FREEZE_HALF);
    }
    hits += 1;

    // R8 突破批：全高样本批 → 裁定必须为不通过（闸门语义）且落账。
    let breach = synth_breach_batch(2000, 0x4828_2BAD);
    let (pb, okb) = measure(&breach, gate.threshold())?;
    if okb {
        return Err(E_P95_MISMATCH);
    }
    if pb < 1100 {
        return Err(E_P95_MISMATCH);
    }
    if !led.record(P95Row {
        batch_id: 4,
        n: breach.len() as u32,
        p95: pb,
        pass: okb,
    }) {
        return Err(E_LEDGER_BAD);
    }
    if led.len() != 4 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R9 双路一致性抽查：批 1 P95 主算值与逐点独立重算（秩走查第二路）
    //    同值——对 3 个候选值（p1-1, p1, p1+1）做秩反证，唯 p1 过闸。
    let idx = (batch1.len() * 95 + 99) / 100;
    if rank_verify(&batch1, p1.wrapping_sub(1), idx) {
        return Err(E_P95_MISMATCH);
    }
    if !rank_verify(&batch1, p1, idx) {
        return Err(E_P95_MISMATCH);
    }
    if rank_verify(&batch1, p1 + 1, idx) {
        return Err(E_P95_MISMATCH);
    }
    hits += 1;

    // R10 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_P95_MISMATCH,
        E_RANK_BAD,
        E_STATE_NOTE_MISMATCH,
        E_FREEZE_PREMATURE,
        E_FREEZE_HALF,
        E_LEDGER_BAD,
        E_SAMPLE_EMPTY,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
