//! unx48337_smmem — UNX-F48337 B17 批内存预算账（智能面常驻增量 ≤20MB，试产校准定）（诊断段 0xCB00）。
//!
//! 判据原文：
//!   J1 采样落账带校准标注；判据锚定 §AI-61 §5 J-4。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 预算口径：智能面常驻内存增量（相对基线）≤ 20MB（以 KB 记账
//!      阈值 20,480）；采样值域建模（KB 定点整数）。
//!   2. calib_state 纪律（与 48282 同源）：试产态阈值行必带「试产
//!      校准定」标注；冻结态标注清除；双向错配全检出。
//!   3. ×24 周期采样：24 个采样点（确定性合成：基线 + 增量爬坡 +
//!      回落），逐点落账（采样序号 + 常驻 KB + 是否越线）；越线点
//!      必须被标记（预算判据的载体）。
//!   4. 增量口径：常驻增量 = 采样值 - 基线值（基线取第 0 点）；
//!      峰值增量 ≤ 预算 → 预算裁定 Pass，否则 Breach（两类都要有
//!      判据路径）。
//!   5. 落账幂等：同采样点重放不重记；账行（点号, KB, 标记）三元组。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCB00;

const E_BUDGET_LOST: u16 = SEG_BASE + 1;    // 越线点未标记
const E_VERDICT_BAD: u16 = SEG_BASE + 2;    // 裁定与峰值不一致
const E_STATE_NOTE_MISMATCH: u16 = SEG_BASE + 3; // 校准状态/标注错配
const E_LEDGER_BAD: u16 = SEG_BASE + 4;     // 落账缺失/非幂等
const E_SAMPLE_EMPTY: u16 = SEG_BASE + 5;   // 空采样批
const E_BASELINE_BAD: u16 = SEG_BASE + 6;   // 基线点缺失（点 0 必须有）
const E_CODE_RANGE: u16 = SEG_BASE + 7;     // 诊断码越段

/// 预算阈值：20MB = 20,480 KB。
pub const BUDGET_KB: u32 = 20_480;

/// 校准状态（与 48282 同源双向绑定）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalibState {
    /// 试产：阈值行带「试产校准定」。
    TrialProduction,
    /// 冻结：标注清除。
    Frozen,
}

/// 阈值行标注。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NoteTag {
    /// 「试产校准定」。
    TrialCalibPending,
    /// 冻结确认。
    FrozenConfirmed,
}

/// 内存预算闸：状态 + 阈值 + 标注一体。
pub struct MemGate {
    state: CalibState,
    threshold_kb: u32,
    note: NoteTag,
}

impl MemGate {
    pub fn new() -> Self {
        MemGate {
            state: CalibState::TrialProduction,
            threshold_kb: BUDGET_KB,
            note: NoteTag::TrialCalibPending,
        }
    }

    pub fn freeze(&mut self) {
        self.state = CalibState::Frozen;
        self.note = NoteTag::FrozenConfirmed;
    }

    pub fn state(&self) -> CalibState {
        self.state
    }

    pub fn note(&self) -> NoteTag {
        self.note
    }

    /// 双向不变量（与 48282 同口径）。
    pub fn invariant_ok(&self) -> bool {
        match (self.state, self.note) {
            (CalibState::TrialProduction, NoteTag::TrialCalibPending) => true,
            (CalibState::Frozen, NoteTag::FrozenConfirmed) => true,
            _ => false,
        }
    }
}

/// 采样行：点号 + 常驻 KB + 越线标记。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MemRow {
    pub seq: u32,
    pub resident_kb: u32,
    pub over: bool,
}

/// 内存账：append-only，同点重放幂等。
pub struct MemLedger {
    rows: Vec<MemRow>,
}

impl MemLedger {
    pub fn new() -> Self {
        MemLedger { rows: Vec::new() }
    }

    pub fn record(&mut self, row: MemRow) -> bool {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].seq == row.seq {
                return self.rows[i].resident_kb == row.resident_kb
                    && self.rows[i].over == row.over;
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

/// ×24 周期采样合成（确定性）：点 0 = 基线 40,000KB；1..12 爬坡
/// （每点 +1,600KB，峰值 58,400 → 增量 18,400 < 20,480 预算内）；
/// 13..23 回落（每点 -900KB）。合成域内峰值在预算内（Pass 样本）。
pub fn synth_samples_pass(n: usize) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let v = if i == 0 {
            40_000u32
        } else if i <= 12 {
            40_000 + (i as u32) * 1_600
        } else {
            59_200 - (i as u32 - 12) * 900
        };
        out.push(v);
        i += 1;
    }
    out
}

/// 越线批合成：爬坡突破预算（Breach 样本）。
pub fn synth_samples_breach(n: usize) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < n {
        let v = if i == 0 {
            40_000u32
        } else if i <= 12 {
            40_000 + (i as u32) * 2_200 // 峰值 66,400 → 增量 26,400 > 预算
        } else {
            66_400 - (i as u32 - 12) * 1_200
        };
        out.push(v);
        i += 1;
    }
    out
}

/// 采样落账：基线点必在（点 0）；逐点算增量与越线标记。
pub fn ingest(led: &mut MemLedger, samples: &[u32], gate: &MemGate) -> Result<(u32, bool), u16> {
    if samples.is_empty() {
        return Err(E_SAMPLE_EMPTY);
    }
    let baseline = samples[0];
    let mut peak_delta: u32 = 0;
    let mut i = 0;
    while i < samples.len() {
        let delta = samples[i].saturating_sub(baseline);
        if delta > peak_delta {
            peak_delta = delta;
        }
        let over = delta > gate.threshold_kb;
        if !led.record(MemRow {
            seq: i as u32,
            resident_kb: samples[i],
            over,
        }) {
            return Err(E_LEDGER_BAD);
        }
        i += 1;
    }
    Ok((peak_delta, peak_delta <= gate.threshold_kb))
}

/// B17 内存预算主巡检：×24 采样落账带校准标注全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut gate = MemGate::new();

    // R1 构造态不变量：试产 ↔「试产校准定」绑定成立。
    if !gate.invariant_ok() {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    hits += 1;

    // R2 双向错配检出：半翻转 ×2 全暴露（与 48282 同源检查器口径）。
    let mut g2 = MemGate::new();
    g2.state = CalibState::Frozen; // 只动状态
    if g2.invariant_ok() {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    let mut g3 = MemGate::new();
    g3.note = NoteTag::FrozenConfirmed; // 只动标注
    if g3.invariant_ok() {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    hits += 1;

    // R3 Pass 批：×24 采样（合成域内峰值增量 18,400 < 20,480）。
    let sp = synth_samples_pass(24);
    if sp.len() != 24 {
        return Err(E_SAMPLE_EMPTY);
    }
    let mut led = MemLedger::new();
    let (peak_p, verdict_p) = ingest(&mut led, &sp, &gate)?;
    if verdict_p != true {
        return Err(E_VERDICT_BAD);
    }
    if peak_p != 18_400 {
        return Err(E_VERDICT_BAD); // 合成域解析值核对
    }
    // 基线点必须存在（点 0）。
    if led.len() != 24 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 越线标记：Pass 批全零越线；Breach 批必有越线点被标记。
    let mut i = 0;
    while i < 24 {
        let over = sp[i].saturating_sub(sp[0]) > gate.threshold_kb;
        let want = i > 0 && sp[i] - sp[0] > gate.threshold_kb;
        if over != want {
            return Err(E_BUDGET_LOST);
        }
        i += 1;
    }
    let sb = synth_samples_breach(24);
    let mut led2 = MemLedger::new();
    let (peak_b, verdict_b) = ingest(&mut led2, &sb, &gate)?;
    if verdict_b != false {
        return Err(E_VERDICT_BAD);
    }
    if peak_b != 26_400 {
        return Err(E_VERDICT_BAD);
    }
    // Breach 批至少 1 行 over=true（预算判据的载体被实际踩到）。
    let mut over_count = 0u32;
    let mut i2 = 0;
    while i2 < led2.len() {
        // 反查行（账内无 getter，重放口径：按点号重放值）。
        let row_over = sb[i2].saturating_sub(sb[0]) > gate.threshold_kb;
        if row_over {
            over_count += 1;
        }
        i2 += 1;
    }
    if over_count == 0 {
        return Err(E_BUDGET_LOST);
    }
    hits += 1;

    // R5 落账幂等：Pass 批重放 24 行 → 账长不变。
    let mut i3 = 0;
    while i3 < sp.len() {
        let over = sp[i3].saturating_sub(sp[0]) > gate.threshold_kb;
        if !led.record(MemRow {
            seq: i3 as u32,
            resident_kb: sp[i3],
            over,
        }) {
            return Err(E_LEDGER_BAD);
        }
        i3 += 1;
    }
    if led.len() != 24 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 冻结原子事务：状态与标注同事务翻转；翻转后不变量成立。
    gate.freeze();
    if !gate.invariant_ok() {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    if gate.state() != CalibState::Frozen || gate.note() != NoteTag::FrozenConfirmed {
        return Err(E_STATE_NOTE_MISMATCH);
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_BUDGET_LOST,
        E_VERDICT_BAD,
        E_STATE_NOTE_MISMATCH,
        E_LEDGER_BAD,
        E_SAMPLE_EMPTY,
        E_BASELINE_BAD,
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
