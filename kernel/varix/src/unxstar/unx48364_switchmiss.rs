//! unx48364_switchmiss — UNX-F48364 切换误操作信号（短时反复中英切换、连点合并失败标记）（诊断段 0xCE60）。
//!
//! 判据原文：
//!   J1 信号 ×10 捕获入账；判据锚定 体验十三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 信号只从事件流来：中英切换误操作不是拍脑袋标签，而是检测器对
//!      （时刻, 切后模式）事件流扫描得出的结论，每条信号可回指到事件区间。
//!   2. 两类硬信号：短时反复切换 = 窗口 WINDOW_MS 内模式翻转 ≥3 次；
//!      连点合并失败 = 间隔小于 MERGE_GAP_MS 的同键连点未被合并成单次
//!      提交（合并器该做的事没做成，独立标记）。
//!   3. ×10 捕获入账：剧本流里埋设 ≥10 处真实信号，检测器必须全部捕获
//!      且恰好按序入账；少捕是漏报，多捕重复是账目污染。
//!   4. 干净流零误报：慢速正常切换（间隔远大于窗口）不得产生任何信号，
//!      否则哨兵不配入账。
//!   5. 账 append-only：同（信号类, 起点）重复登记幂等不重记；检测器两轮
//!      运行指纹必须一致（确定性，无真随机）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCE60;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CAPTURE_SHORT: u16 = SEG_BASE + 1; // 捕获信号不足 10 条
const E_SPAN_OVERRUN: u16 = SEG_BASE + 2;  // 信号区间超出窗口上限
const E_LEDGER_FAULT: u16 = SEG_BASE + 3;  // 入账数量/幂等性违规
const E_FALSE_POSITIVE: u16 = SEG_BASE + 4; // 干净流误报
const E_MERGE_FAULT: u16 = SEG_BASE + 5;   // 连点合并结果不符
const E_DETERMINISM_FAULT: u16 = SEG_BASE + 6; // 两轮扫描指纹不一致
const E_MISS_UNDETECTED: u16 = SEG_BASE + 7; // 埋设信号漏报
const E_CODE_RANGE: u16 = SEG_BASE + 8;    // 诊断码越段

/// 反复切换判定窗口（毫秒）。
pub const WINDOW_MS: u32 = 400;

/// 连点合并失败判定间隔（毫秒）。
pub const MERGE_GAP_MS: u32 = 80;

/// 触发反复切换信号所需的最少翻转次数。
pub const TOGGLE_TRIGGER: u32 = 3;

/// 切换后模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Han,
    Latin,
}

/// 切换事件流事件：seq 单调，ts 毫秒时刻，mode 为切换后模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SwitchEvent {
    pub seq: u32,
    pub ts: u32,
    pub mode: Mode,
}

/// 信号类别。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    /// 短时反复中英切换。
    RapidToggle,
    /// 连点合并失败标记。
    MergeFail,
}

/// 检出的误操作信号：可回指事件区间 [first_seq, last_seq]。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MissSignal {
    pub kind: SignalKind,
    pub first_seq: u32,
    pub last_seq: u32,
    /// 翻转次数或连点个数。
    pub count: u32,
}

/// 误操作信号账：append-only，同（类, 起点）幂等不重记。
pub struct MissLedger {
    entries: Vec<MissSignal>,
}

impl MissLedger {
    pub fn new() -> Self {
        MissLedger {
            entries: Vec::new(),
        }
    }

    /// 返回 true = 新记账或幂等重放；false = 同键位裁决冲突。
    pub fn record(&mut self, s: MissSignal) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            let t = &self.entries[i];
            if t.kind == s.kind && t.first_seq == s.first_seq {
                return t.last_seq == s.last_seq && t.count == s.count;
            }
            i += 1;
        }
        self.entries.push(s);
        true
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[MissSignal] {
        &self.entries
    }
}

/// 确定性线性同余发生器（剧本生成统一入口，禁真随机）。
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

/// 信号检测器：对切换事件流做单遍扫描，输出全部硬信号。
/// 返回序 = 连点合并失败标记在前，折叠后的反复切换信号在后。
pub fn detect(events: &[SwitchEvent]) -> Vec<MissSignal> {
    // 扫描一：短时反复切换——以每事件为窗口起点，数窗口内模式翻转。
    let mut cands: Vec<MissSignal> = Vec::new();
    let mut i = 0usize;
    while i < events.len() {
        let mut flips: u32 = 0;
        let mut last_mode = events[i].mode;
        let mut j = i + 1;
        while j < events.len() && events[j].ts - events[i].ts <= WINDOW_MS {
            if events[j].mode != last_mode {
                flips += 1;
                last_mode = events[j].mode;
            }
            j += 1;
        }
        if flips >= TOGGLE_TRIGGER {
            cands.push(MissSignal {
                kind: SignalKind::RapidToggle,
                first_seq: events[i].seq,
                last_seq: events[j - 1].seq,
                count: flips,
            });
        }
        i += 1;
    }
    // 折叠重叠窗口：仅保留起点互不包含的信号（起点间隔 > WINDOW_MS）。
    let mut folded: Vec<MissSignal> = Vec::new();
    let mut i = 0usize;
    while i < cands.len() {
        let s = cands[i];
        let mut overlapped = false;
        let mut j = 0usize;
        while j < folded.len() {
            if s.first_seq - folded[j].first_seq < 0x1000
                && s.last_seq <= folded[j].last_seq
            {
                overlapped = true;
                break;
            }
            j += 1;
        }
        if !overlapped {
            folded.push(s);
        }
        i += 1;
    }
    let mut sigs: Vec<MissSignal> = Vec::new();
    // 扫描二：连点合并失败——相邻两事件间隔小于 MERGE_GAP_MS 且未合并。
    let mut i = 1usize;
    while i < events.len() {
        let gap = events[i].ts - events[i - 1].ts;
        if gap < MERGE_GAP_MS && events[i].mode == events[i - 1].mode {
            sigs.push(MissSignal {
                kind: SignalKind::MergeFail,
                first_seq: events[i - 1].seq,
                last_seq: events[i].seq,
                count: 2,
            });
        }
        i += 1;
    }
    let mut i = 0usize;
    while i < folded.len() {
        sigs.push(folded[i]);
        i += 1;
    }
    sigs
}

/// 埋设剧本：10 处短时反复切换簇 + 2 处连点簇 + 大段干净流。
/// 返回（事件流，埋设的反复切换簇数）。
pub fn script(seed: u32) -> (Vec<SwitchEvent>, u32) {
    let mut lc = Lcg::new(seed);
    let mut evs: Vec<SwitchEvent> = Vec::new();
    let mut seq: u32 = 0;
    let mut ts: u32 = 0;
    let mut buried: u32 = 0;
    // 10 个反复切换簇：每簇 5 次翻转、间隔 60ms（窗口 400ms 内）。
    let mut c = 0u32;
    while c < 10 {
        let mut k = 0u32;
        while k < 5 {
            evs.push(SwitchEvent {
                seq,
                ts,
                mode: if k % 2 == 0 { Mode::Han } else { Mode::Latin },
            });
            seq += 1;
            ts += 60;
            k += 1;
        }
        buried += 1;
        ts += 1200; // 簇间距远大于窗口
        c += 1;
    }
    // 2 个连点簇：间隔 40ms 的同模式双击（应合并而未合并）。
    let mut c = 0u32;
    while c < 2 {
        evs.push(SwitchEvent {
            seq,
            ts,
            mode: Mode::Latin,
        });
        seq += 1;
        ts += 40;
        evs.push(SwitchEvent {
            seq,
            ts,
            mode: Mode::Latin,
        });
        seq += 1;
        ts += 1000;
        c += 1;
    }
    // 干净流：慢速正常切换，间隔 900ms 起步，夹杂 LCG 抖动但不进窗口。
    let mut k = 0u32;
    while k < 60 {
        ts += 900 + lc.next() % 500;
        evs.push(SwitchEvent {
            seq,
            ts,
            mode: if lc.next() % 2 == 0 { Mode::Han } else { Mode::Latin },
        });
        seq += 1;
        k += 1;
    }
    (evs, buried)
}

/// 信号指纹：FNV-1a 折叠（确定性自查用）。
pub fn fingerprint(sigs: &[MissSignal]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < sigs.len() {
        h ^= sigs[i].kind as u8 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= sigs[i].first_seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= sigs[i].last_seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= sigs[i].count as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 自例程判据：逐条落实 J1 的全部断言（信号 ×10 捕获入账）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let (evs, buried) = script(0x4836_4001);

    // R1 ×10 捕获：反复切换信号必须 ≥10 条，与埋设簇数一致。
    let sigs = detect(&evs);
    let mut toggles = 0u32;
    let mut merges = 0u32;
    let mut i = 0usize;
    while i < sigs.len() {
        if sigs[i].kind == SignalKind::RapidToggle {
            toggles += 1;
        } else {
            merges += 1;
        }
        i += 1;
    }
    if toggles < 10 || toggles != buried {
        return Err(E_CAPTURE_SHORT);
    }
    hits += 1;

    // R2 区间不越窗：每条反复切换信号的 ts 跨度必须 ≤ WINDOW_MS。
    i = 0;
    while i < sigs.len() {
        let s = sigs[i];
        if s.kind == SignalKind::RapidToggle {
            let t0 = s.first_seq as usize;
            let t1 = s.last_seq as usize;
            if t1 >= evs.len() || evs[t1].ts - evs[t0].ts > WINDOW_MS {
                return Err(E_SPAN_OVERRUN);
            }
        }
        i += 1;
    }
    hits += 1;

    // R3 入账：全部信号按序入账，账长与检出数一致，幂等重放不重记。
    let mut led = MissLedger::new();
    i = 0;
    while i < sigs.len() {
        if !led.record(sigs[i]) {
            return Err(E_LEDGER_FAULT);
        }
        i += 1;
    }
    if led.len() != sigs.len() || led.len() < 12 {
        return Err(E_LEDGER_FAULT);
    }
    if !led.record(sigs[0]) || led.len() != sigs.len() {
        return Err(E_LEDGER_FAULT);
    }
    hits += 1;

    // R4 干净流零误报：只取剧本尾部干净段，必须零信号。
    let clean_start = evs.len() - 60;
    let mut clean: Vec<SwitchEvent> = Vec::new();
    i = clean_start;
    while i < evs.len() {
        clean.push(evs[i]);
        i += 1;
    }
    if detect(&clean).len() != 0 {
        return Err(E_FALSE_POSITIVE);
    }
    hits += 1;

    // R5 连点合并失败标记：两处连点簇必须各产出一条 MergeFail。
    if merges < 2 {
        return Err(E_MERGE_FAULT);
    }
    // 合并器正常路径自查：间隔 40ms 的双击喂给合并器（模拟已合并）
    // 后，流上不再残留 MergeFail。
    let mut merged_stream: Vec<SwitchEvent> = Vec::new();
    let mut c = 0u32;
    let mut ts = 0u32;
    let mut seq = 0u32;
    while c < 2 {
        merged_stream.push(SwitchEvent {
            seq,
            ts,
            mode: Mode::Latin,
        });
        seq += 1;
        ts += 1000; // 合并后的单次提交
        c += 1;
    }
    let mut k = 0u32;
    while k < 20 {
        ts += 1000;
        merged_stream.push(SwitchEvent {
            seq,
            ts,
            mode: Mode::Han,
        });
        seq += 1;
        k += 1;
    }
    let mut m = 0usize;
    let msig = detect(&merged_stream);
    while m < msig.len() {
        if msig[m].kind == SignalKind::MergeFail {
            return Err(E_MERGE_FAULT);
        }
        m += 1;
    }
    hits += 1;

    // R6 确定性：同流两轮扫描指纹必须一致。
    let fp1 = fingerprint(&sigs);
    let sigs2 = detect(&evs);
    if fingerprint(&sigs2) != fp1 {
        return Err(E_DETERMINISM_FAULT);
    }
    hits += 1;

    // R7 漏报自查：抽掉一个切换簇（保留 9 个）后，检出数必须减一。
    let mut trimmed: Vec<SwitchEvent> = Vec::new();
    i = 0;
    while i < evs.len() {
        // 剔除第一簇（seq 0..=4）。
        if evs[i].seq < 5 {
            i += 1;
            continue;
        }
        trimmed.push(evs[i]);
        i += 1;
    }
    let mut t2 = 0u32;
    let tsig = detect(&trimmed);
    let mut m = 0usize;
    while m < tsig.len() {
        if tsig[m].kind == SignalKind::RapidToggle {
            t2 += 1;
        }
        m += 1;
    }
    if t2 != toggles - 1 {
        return Err(E_MISS_UNDETECTED);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_CAPTURE_SHORT,
        E_SPAN_OVERRUN,
        E_LEDGER_FAULT,
        E_FALSE_POSITIVE,
        E_MERGE_FAULT,
        E_DETERMINISM_FAULT,
        E_MISS_UNDETECTED,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
