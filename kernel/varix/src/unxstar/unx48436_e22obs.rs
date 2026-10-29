//! UNX-F48436 —— E22 观测埋点（截断/降级/越界/合并四点）
//! 判据原文：UNX-F48436-J1 长串管线必须在四个观测点埋桩：截断（TRUNC）、
//! 降级（DEGRADE）、越界（OOB）、合并（MERGE）；四类事件互斥可分、计数守恒、
//! 越限落环形账时丢弃数必须记账、时间戳单调、报告行四段齐全。
//! 判据落地（drill 场景）：R1 四点闭集；R2 四点各记一次计数精确；R3 未知事件拒收；
//! R4 环形账越限丢弃记账守恒；R5 时间戳单调；R6 报告四段齐全互异；
//! R7 计数守恒恒等式；R8 复位归零；R9 指纹稳定；R10 事件注记非零守恒。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDAA0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KIND: u16 = SEG_BASE + 1; // 事件类别越出四点闭集
const E_COUNT: u16 = SEG_BASE + 2; // 计数与入账次数不符
const E_CAP: u16 = SEG_BASE + 3; // 环形账越限未按丢弃记账
const E_CONSERV: u16 = SEG_BASE + 4; // 守恒破缺（入账+丢弃 ≠ 总数）
const E_ORDER: u16 = SEG_BASE + 5; // 时间戳非单调
const E_REPORT: u16 = SEG_BASE + 6; // 报告缺段
const E_RESET: u16 = SEG_BASE + 7; // 复位后仍有残留计数
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_NOTE: u16 = SEG_BASE + 9; // 注记丢失
const E_MUT: u16 = SEG_BASE + 10; // 四点互斥破缺（同刻同源双记）

/// 四点事件类别：1 截断 / 2 降级 / 3 越界 / 4 合并。
pub const KIND_TRUNC: u8 = 1;
pub const KIND_DEGRADE: u8 = 2;
pub const KIND_OOB: u8 = 3;
pub const KIND_MERGE: u8 = 4;

/// 事件类别显示名（下标 = 类别-1）。
pub const KIND_NAMES: [&str; 4] = ["trunc", "degrade", "oob", "merge"];

/// 环形账容量。
pub const LEDGER_CAP: usize = 8;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 单条观测事件：类别 / 时间戳 / 注记（来源键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEvent {
    pub kind: u8,
    pub at: u32,
    pub note: u32,
}

/// 观测总账：四点计数 + 环形事件账 + 丢弃数。
pub struct ObsLedger {
    pub counts: [u32; 4],
    pub log: Vec<ObsEvent>,
    pub dropped: u32,
}

impl ObsLedger {
    pub fn new() -> ObsLedger {
        ObsLedger {
            counts: [0u32; 4],
            log: vec![],
            dropped: 0,
        }
    }

    /// 入账：类别必须落在四点闭集，账满则丢弃并记账。
    pub fn emit(&mut self, kind: u8, at: u32, note: u32) -> Result<(), u16> {
        if kind < KIND_TRUNC || kind > KIND_MERGE {
            return Err(E_KIND);
        }
        self.counts[(kind - 1) as usize] += 1;
        if self.log.len() < LEDGER_CAP {
            self.log.push(ObsEvent { kind, at, note });
        } else {
            self.dropped += 1;
        }
        Ok(())
    }

    /// 守恒恒等式：四点计数之和 == 账内条数 + 丢弃数。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < 4 {
            sum += self.counts[i];
            i += 1;
        }
        sum == self.log.len() as u32 + self.dropped
    }

    /// 时间戳单调：账内 at 非降序。
    pub fn monotonic(&self) -> bool {
        let mut i = 1usize;
        while i < self.log.len() {
            if self.log[i - 1].at > self.log[i].at {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 复位：计数/账/丢弃全部归零。
    pub fn reset(&mut self) {
        self.counts = [0u32; 4];
        self.log.clear();
        self.dropped = 0;
    }

    /// 四点互斥：同一时间戳下同源注记不得双记（同 at 同 note 只允许一条）。
    pub fn exclusive(&self) -> bool {
        let mut i = 0usize;
        while i < self.log.len() {
            let mut j = i + 1;
            while j < self.log.len() {
                if self.log[i].at == self.log[j].at && self.log[i].note == self.log[j].note {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 报告行：trunc=<n> degrade=<n> oob=<n> merge=<n>。
pub fn report(l: &ObsLedger) -> String {
    let mut s = String::new();
    s.push_str("trunc=");
    s.push_str(&l.counts[0].to_string());
    s.push_str(" degrade=");
    s.push_str(&l.counts[1].to_string());
    s.push_str(" oob=");
    s.push_str(&l.counts[2].to_string());
    s.push_str(" merge=");
    s.push_str(&l.counts[3].to_string());
    s
}

/// 账指纹：四点计数 + 账内事件 + 丢弃数串联。
pub fn ledger_fp(l: &ObsLedger) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < 4 {
        buf.extend_from_slice(&l.counts[i].to_le_bytes());
        i += 1;
    }
    let mut j = 0usize;
    while j < l.log.len() {
        buf.push(l.log[j].kind);
        buf.extend_from_slice(&l.log[j].at.to_le_bytes());
        buf.extend_from_slice(&l.log[j].note.to_le_bytes());
        j += 1;
    }
    buf.extend_from_slice(&l.dropped.to_le_bytes());
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 四点闭集：类别 1..=4 与显示名一一对应，0/5 越集。
    let mut k = KIND_TRUNC;
    while k <= KIND_MERGE {
        if KIND_NAMES[(k - 1) as usize].is_empty() {
            return Err(E_KIND);
        }
        k += 1;
    }
    let mut probe = ObsLedger::new();
    if probe.emit(0, 1, 1).is_ok() || probe.emit(5, 1, 1).is_ok() {
        return Err(E_KIND);
    }
    hits += 1;

    // R2 四点各记一次：计数精确、类别名可分辨。
    let mut led = ObsLedger::new();
    led.emit(KIND_TRUNC, 10, 101)?;
    led.emit(KIND_DEGRADE, 20, 102)?;
    led.emit(KIND_OOB, 30, 103)?;
    led.emit(KIND_MERGE, 40, 104)?;
    if led.counts[0] != 1 || led.counts[1] != 1 || led.counts[2] != 1 || led.counts[3] != 1 {
        return Err(E_COUNT);
    }
    hits += 1;

    // R3 未知事件拒收：诊断码恰为 E_KIND。
    match led.emit(9, 50, 105) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_KIND),
    }

    // R4 环形账越限：再入 6 条，账满后 2 条落丢弃，且丢弃记账。
    led.emit(KIND_TRUNC, 50, 201)?;
    led.emit(KIND_DEGRADE, 60, 202)?;
    led.emit(KIND_OOB, 70, 203)?;
    led.emit(KIND_MERGE, 80, 204)?;
    led.emit(KIND_TRUNC, 90, 205)?;
    led.emit(KIND_DEGRADE, 100, 206)?;
    if led.log.len() != LEDGER_CAP {
        return Err(E_CAP);
    }
    if led.dropped != 2 {
        return Err(E_CAP);
    }
    hits += 1;

    // R5 时间戳单调：当前账非降序；乱序注入须被检出。
    if !led.monotonic() {
        return Err(E_ORDER);
    }
    let mut bad = ObsLedger::new();
    bad.log.push(ObsEvent { kind: KIND_TRUNC, at: 20, note: 1 });
    bad.log.push(ObsEvent { kind: KIND_TRUNC, at: 10, note: 2 });
    if bad.monotonic() {
        return Err(E_ORDER);
    }
    hits += 1;

    // R6 报告四段齐全互异：trunc=/degrade=/oob=/merge= 前缀全在。
    let rep = report(&led);
    if !has_kw(rep.as_bytes(), b"trunc=")
        || !has_kw(rep.as_bytes(), b"degrade=")
        || !has_kw(rep.as_bytes(), b"oob=")
        || !has_kw(rep.as_bytes(), b"merge=")
    {
        return Err(E_REPORT);
    }
    hits += 1;

    // R7 守恒恒等式：计数和 == 账内 + 丢弃。
    if !led.conserved() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R8 复位归零：复位后四点计数、账长、丢弃数全零。
    led.reset();
    if led.counts != [0u32; 4] || !led.log.is_empty() || led.dropped != 0 {
        return Err(E_RESET);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let mut fp_led = ObsLedger::new();
    fp_led.emit(KIND_MERGE, 7, 77)?;
    let f1 = ledger_fp(&fp_led);
    let f2 = ledger_fp(&fp_led);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 注记守恒 + 互斥：账内注记逐一可回放，同刻同源无双记。
    let mut ex = ObsLedger::new();
    let mut lcg = Lcg::new(0x48436);
    let mut t = 100u32;
    let mut idx = 0u32;
    while idx < 4 {
        let kind = KIND_TRUNC + (lcg.below(4) as u8);
        ex.emit(kind, t, 900 + idx)?;
        t += 10;
        idx += 1;
    }
    if !ex.exclusive() {
        return Err(E_MUT);
    }
    let mut seen: [bool; 4] = [false; 4];
    let mut i = 0usize;
    while i < ex.log.len() {
        let note = ex.log[i].note;
        if note < 900 || note >= 904 {
            return Err(E_NOTE);
        }
        seen[(note - 900) as usize] = true;
        i += 1;
    }
    let mut si = 0usize;
    while si < 4 {
        if !seen[si] {
            return Err(E_NOTE);
        }
        si += 1;
    }
    if !ex.conserved() {
        return Err(E_CONSERV);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
