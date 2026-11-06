//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18558 · 档案体验日志埋点（变更/冲突/回退全事件）· 0x80740 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18558-J1 三类事件（变更/冲突/回退）全埋点断言，异步
//!       批量落盘不阻塞
//! 本模块结构清单：
//!   1. 冻结常量层：3 类事件闭集（0=变更/1=冲突/2=回退）、环形缓冲
//!      16 槽、批量落盘步长 4、代数闭集、seq 单调闸；
//!   2. 诊断码闭集 ElCode（code() = SEG_BASE + n，0x80740 段窗口）；
//!   3. 数据结构一（事件环）：EventRing——[Option<EventRec>;16]
//!      环形缓冲 + head/tail/len/drops，满环丢弃但丢弃入账（不静默）；
//!   4. 数据结构二（埋点账）：InstrumentLedger——三类事件独立计数
//!      + 已落盘计数 + 批次数 + 丢弃计数，守恒律：
//!      发出 = 已落盘 + 环内滞留 + 丢弃（不生不灭）；
//!   5. 埋点核 emit：域动作（变更/冲突/回退）必发对应类事件——
//!      全埋点断言 = 三类计数 == 对应域动作数；seq 单调 +1，乱序/
//!      重复 seq 显式拒（注入面）；
//!   6. 批量落盘核 flush_batch：每次至多取 4 条（冻结步长），环锁
//!      不持有跨调用——模拟异步批量：emit 永不等待（满环即丢弃
//!      入账），落盘永不阻塞 emit（两核零共享锁）；
//!   7. 注入核 3 类×10 轮：未知事件类 / 重复 seq / 乱序 seq，必拒；
//!   8. 黄金全链 golden_run + run_unx18558_checks：15 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么满环选择「丢弃入账」而不是阻塞：埋点在内核路径上，阻塞
//! 域动作等于性能事故；丢弃必须入账（drops 计数）才能在观测面上
//! 补偿——守恒律「发出=落盘+滞留+丢弃」保证每条事件都有去处。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80740，AI-VX11-1105-G1·A16 批 08 独占窗口）。
pub const SEG_BASE: u32 = 0x80740;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 事件类闭集（冻结：0=变更/1=冲突/2=回退，对应三类域动作）。
pub const KIND_CHANGE: u8 = 0;
pub const KIND_CONFLICT: u8 = 1;
pub const KIND_FALLBACK: u8 = 2;
pub const KIND_N: usize = 3;
/// 环形缓冲容量（冻结：16 槽）。
pub const RING_CAP: usize = 16;
/// 批量落盘步长（冻结：4 条/批）。
pub const BATCH_SIZE: usize = 4;
/// 代数闭集上界（冻结：代数从 1 起，单调不回落）。
pub const GEN_MAX: u32 = 999;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElCode {
    /// 事件类未知
    KindUnknown,
    /// seq 重复
    SeqDup,
    /// seq 乱序（小于已见最大值）
    SeqRegress,
    /// 环账损坏（len/head/tail 失配）
    RingCorrupt,
    /// 埋点缺失（域动作数≠事件发出数）
    MissingInstrument,
    /// 守恒律破（发出≠落盘+滞留+丢弃）
    ConservationBroken,
    /// 批量步长违规（单批>4）
    BatchTooBig,
    /// 双跑不一致
    NotReproducible,
}

impl ElCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ElCode::KindUnknown => "事件类未知：不在 变更/冲突/回退 闭集",
            ElCode::SeqDup => "seq 重复：事件序号不唯一",
            ElCode::SeqRegress => "seq 乱序：序号小于已见最大值",
            ElCode::RingCorrupt => "环账损坏：len/head/tail 失配",
            ElCode::MissingInstrument => "埋点缺失：域动作数≠事件发出数",
            ElCode::ConservationBroken => "守恒律破：发出≠落盘+滞留+丢弃",
            ElCode::BatchTooBig => "批量步长违规：单批超过 4 条",
            ElCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [ElCode; 8] = [
        ElCode::KindUnknown,
        ElCode::SeqDup,
        ElCode::SeqRegress,
        ElCode::RingCorrupt,
        ElCode::MissingInstrument,
        ElCode::ConservationBroken,
        ElCode::BatchTooBig,
        ElCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ElCode::ALL.len() {
        if ElCode::ALL[i].code() == code {
            return ElCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80740 段闭集内"
}

// ── 事件记录 ────────────────────────────────────────────────────
/// 一条事件：序号 + 类 + 诊断码 + 代数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventRec {
    pub seq: u32,
    pub kind: u8,
    pub code: u32,
    pub gen: u32,
}

// ── 数据结构一：事件环 ──────────────────────────────────────────
/// 环形缓冲：head=出队位、tail=入队位、len=滞留数。
/// 为什么环形数组不用 Vec：固定槽位的环让「满」成为显式状态，
/// 丢弃策略有账可依；Vec 的隐式扩容在 no_std 面上是静默成本。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRing {
    buf: [Option<EventRec>; RING_CAP],
    head: usize,
    tail: usize,
    len: usize,
    pub drops: u32,
    /// 已见最大 seq（单调闸）。
    last_seq: u32,
}

impl EventRing {
    /// 新建空环。
    pub fn new() -> EventRing {
        EventRing {
            buf: [None; RING_CAP],
            head: 0,
            tail: 0,
            len: 0,
            drops: 0,
            last_seq: 0,
        }
    }
    /// 入环：满环丢弃入账（不阻塞、不 panic）；seq 单调闸在此拦。
    pub fn push(&mut self, rec: EventRec) -> Result<bool, u32> {
        // 事件类闭集闸。
        if rec.kind >= KIND_N as u8 {
            return Err(ElCode::KindUnknown.code());
        }
        // seq 单调闸：重复与乱序都拒（注入面）。
        if rec.seq == self.last_seq {
            return Err(ElCode::SeqDup.code());
        }
        if rec.seq < self.last_seq {
            return Err(ElCode::SeqRegress.code());
        }
        self.last_seq = rec.seq;
        if self.len >= RING_CAP {
            // 满环：丢弃入账，仍返回 Ok（非阻塞语义）。
            self.drops = self.drops.saturating_add(1);
            return Ok(false);
        }
        self.buf[self.tail] = Some(rec);
        self.tail = (self.tail + 1) % RING_CAP;
        self.len += 1;
        Ok(true)
    }
    /// 出环：弹最旧一条（空环返回 None）。
    pub fn pop(&mut self) -> Option<EventRec> {
        if self.len == 0 {
            return None;
        }
        let rec = self.buf.get(self.head).copied().unwrap_or(None);
        self.buf[self.head] = None;
        self.head = (self.head + 1) % RING_CAP;
        self.len = self.len.saturating_sub(1);
        rec
    }
    /// 滞留数。
    pub fn pending(&self) -> usize {
        self.len
    }
    /// 丢弃数。
    pub fn dropped(&self) -> u32 {
        self.drops
    }
    /// 环账守恒：len ≤ CAP 且 head/tail 与 len 自洽。
    pub fn ring_ok(&self) -> bool {
        self.len <= RING_CAP
            && (self.head + self.len) % RING_CAP == self.tail
    }
}

// ── 数据结构二：埋点账 ──────────────────────────────────────────
/// 埋点账：三类事件发出计数（=对应域动作数）+ 落盘/批次/丢弃。
/// 守恒律：三类发出和 == 落盘 + 滞留 + 丢弃。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentLedger {
    pub emitted: [u32; KIND_N],
    pub flushed: u32,
    pub batches: u32,
    pub drop_total: u32,
}

impl InstrumentLedger {
    /// 新账。
    pub fn new() -> InstrumentLedger {
        InstrumentLedger { emitted: [0; KIND_N], flushed: 0, batches: 0, drop_total: 0 }
    }
    /// 发出总数。
    pub fn emitted_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < KIND_N {
            n = n.saturating_add(self.emitted[i]);
            i += 1;
        }
        n
    }
    /// 守恒核：发出 = 落盘 + 滞留 + 丢弃。
    pub fn conserve(&self, pending: usize) -> Result<(), u32> {
        let rhs = self
            .flushed
            .saturating_add(pending as u32)
            .saturating_add(self.drop_total);
        if self.emitted_total() != rhs {
            return Err(ElCode::ConservationBroken.code());
        }
        Ok(())
    }
    /// 全埋点断言：三类各有发出且发出数==传入的域动作数组。
    pub fn all_instrumented(&self, change: u32, conflict: u32, fallback: u32) -> bool {
        self.emitted[0] == change && self.emitted[1] == conflict && self.emitted[2] == fallback
    }
}

// ── 埋点核（域动作 → 事件） ─────────────────────────────────────
/// 域动作记录：一次变更/冲突/回退各计一次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainActions {
    pub change: u32,
    pub conflict: u32,
    pub fallback: u32,
}

/// 埋点发射：域动作逐条转事件入环 + 埋点账逐类计数。
/// seq 由调用方从 1 起单调给出（重复/乱序由环闸拒）。
pub fn emit_actions(
    ring: &mut EventRing,
    ledger: &mut InstrumentLedger,
    acts: &DomainActions,
    gen: u32,
) -> Result<(), u32> {
    let mut seq: u32 = ring.last_seq.saturating_add(1);
    // 变更类。
    let mut i = 0;
    while i < acts.change {
        let rec = EventRec { seq, kind: KIND_CHANGE, code: SEG_BASE, gen };
        let _ = ring.push(rec)?; // 未知类/乱序在此必拒
        ledger.emitted[0] = ledger.emitted[0].saturating_add(1);
        seq = seq.saturating_add(1);
        i += 1;
    }
    // 冲突类。
    let mut j = 0;
    while j < acts.conflict {
        let rec = EventRec { seq, kind: KIND_CONFLICT, code: SEG_BASE + 1, gen };
        let _ = ring.push(rec)?;
        ledger.emitted[1] = ledger.emitted[1].saturating_add(1);
        seq = seq.saturating_add(1);
        j += 1;
    }
    // 回退类。
    let mut k = 0;
    while k < acts.fallback {
        let rec = EventRec { seq, kind: KIND_FALLBACK, code: SEG_BASE + 2, gen };
        let _ = ring.push(rec)?;
        ledger.emitted[2] = ledger.emitted[2].saturating_add(1);
        seq = seq.saturating_add(1);
        k += 1;
    }
    // 丢弃账同步（环丢弃数并入账）。
    ledger.drop_total = ring.dropped();
    Ok(())
}

// ── 批量落盘核（异步批量，不阻塞） ──────────────────────────────
/// 批量落盘：每调用一次取至多 BATCH_SIZE 条；调用方循环调用至
/// 环空。批次数与落盘数入账；单批永不超步长（BatchTooBig 由构造
/// 保证——步长是常量，此处复核断言口径）。
pub fn flush_batch(
    ring: &mut EventRing,
    ledger: &mut InstrumentLedger,
) -> Result<usize, u32> {
    let mut took = 0usize;
    let mut i = 0;
    while i < BATCH_SIZE {
        match ring.pop() {
            Some(_rec) => {
                took += 1;
                ledger.flushed = ledger.flushed.saturating_add(1);
            }
            None => break,
        }
        i += 1;
    }
    if took > BATCH_SIZE {
        return Err(ElCode::BatchTooBig.code());
    }
    if took > 0 {
        ledger.batches = ledger.batches.saturating_add(1);
    }
    if !ring.ring_ok() {
        return Err(ElCode::RingCorrupt.code());
    }
    Ok(took)
}

/// 落盘至环空（模拟异步批量泵：循环调 flush_batch）。
pub fn flush_all(ring: &mut EventRing, ledger: &mut InstrumentLedger) -> Result<usize, u32> {
    let mut total = 0usize;
    let mut guard = 0;
    while ring.pending() > 0 {
        let n = flush_batch(ring, ledger)?;
        total = total.saturating_add(n);
        guard += 1;
        if guard > RING_CAP * 4 {
            return Err(ElCode::RingCorrupt.code());
        }
    }
    Ok(total)
}

// ── 注入核（3 类坏样本，逐类必拒） ──────────────────────────────
/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全抓判据。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] != rounds || self.caught[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检数（恒零判据）。
    pub fn missed(&self) -> u32 {
        let mut t: u32 = 0;
        let mut c: u32 = 0;
        let mut i = 0;
        while i < 3 {
            t = t.saturating_add(self.tries[i]);
            c = c.saturating_add(self.caught[i]);
            i += 1;
        }
        t.saturating_sub(c)
    }
}

/// 注入轮：未知事件类 / 重复 seq / 乱序 seq 三类各一轮。
pub fn inject_round(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：未知事件类（kind=7）。
        let mut rg1 = EventRing::new();
        led.record(0, rg1.push(EventRec { seq: 1, kind: 7, code: 0, gen: 1 }).is_err());
        // 类 2：重复 seq。
        let mut rg2 = EventRing::new();
        let _ = rg2.push(EventRec { seq: 1, kind: 0, code: 0, gen: 1 });
        led.record(1, rg2.push(EventRec { seq: 1, kind: 1, code: 0, gen: 1 }).is_err());
        // 类 3：乱序 seq（2 后发 1）。
        let mut rg3 = EventRing::new();
        let _ = rg3.push(EventRec { seq: 2, kind: 0, code: 0, gen: 1 });
        led.record(2, rg3.push(EventRec { seq: 1, kind: 1, code: 0, gen: 1 }).is_err());
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：域动作 (变更5/冲突3/回退2) 全埋点 → 批量落盘至环空 →
/// 守恒律 → 非阻塞断言（20 条溢出：16 滞留上限 + 4 丢弃入账）→
/// 全埋点断言（三类计数==域动作数）→ 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 场景一：三类全埋点 + 批量落盘守恒。
    let mut ring = EventRing::new();
    let mut led = InstrumentLedger::new();
    let acts = DomainActions { change: 5, conflict: 3, fallback: 2 };
    emit_actions(&mut ring, &mut led, &acts, 1)?;
    if !led.all_instrumented(5, 3, 2) {
        return Err(ElCode::MissingInstrument.code());
    }
    if !ring.ring_ok() {
        return Err(ElCode::RingCorrupt.code());
    }
    let drained = flush_all(&mut ring, &mut led)?;
    if drained != 10 {
        return Err(ElCode::ConservationBroken.code());
    }
    led.conserve(ring.pending())?;
    if ring.pending() != 0 {
        return Err(ElCode::RingCorrupt.code());
    }
    // 批次数：10 条 / 步长 4 → 3 批（4+4+2）。
    if led.batches != 3 {
        return Err(ElCode::BatchTooBig.code());
    }
    // 场景二：非阻塞断言——发 20 条（>16 槽），4 条丢弃入账不阻塞。
    let mut ring2 = EventRing::new();
    let mut led2 = InstrumentLedger::new();
    let acts2 = DomainActions { change: 12, conflict: 5, fallback: 3 };
    emit_actions(&mut ring2, &mut led2, &acts2, 2)?;
    if ring2.dropped() != 4 {
        return Err(ElCode::ConservationBroken.code());
    }
    if ring2.pending() != RING_CAP {
        return Err(ElCode::RingCorrupt.code());
    }
    led2.conserve(ring2.pending())?;
    Ok((led.emitted_total(), led.flushed, ring2.dropped()))
}

/// 探针三序（埋点+落盘/空环落盘/满环丢弃）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let mut ring = EventRing::new();
    let mut led = InstrumentLedger::new();
    let _ = emit_actions(&mut ring, &mut led, &DomainActions { change: 2, conflict: 1, fallback: 1 }, 1);
    let _ = flush_all(&mut ring, &mut led);
    Ok(led.flushed)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空环落盘：no-op，0 条 0 批。
    let mut ring = EventRing::new();
    let mut led = InstrumentLedger::new();
    let n = flush_all(&mut ring, &mut led)?;
    if n != 0 || led.batches != 0 {
        return Err(ElCode::RingCorrupt.code());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 满环丢弃：第 17 条起丢弃入账，push 仍 Ok（非阻塞）。
    let mut ring = EventRing::new();
    let mut s = 1u32;
    let mut i = 0;
    while i < RING_CAP + 2 {
        let rec = EventRec { seq: s, kind: KIND_CHANGE, code: 0, gen: 1 };
        let pushed = ring.push(rec)?;
        if i >= RING_CAP && pushed {
            return Err(ElCode::RingCorrupt.code());
        }
        s = s.saturating_add(1);
        i += 1;
    }
    if ring.dropped() != 2 || ring.pending() != RING_CAP {
        return Err(ElCode::RingCorrupt.code());
    }
    Ok(())
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ElCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ElCode::ALL.len() {
        let v = ElCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < ElCode::ALL.len() {
        if ElCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：三类事件全埋点 + 异步批量落盘不阻塞。
    let g = golden_run();
    let g_ok = match &g {
        Ok((em, fl, dr)) => *em == 10 && *fl == 10 && *dr == 4,
        Err(_) => false,
    };
    v.push(("J1-三类事件全埋点+批量落盘", g_ok, format!("{:?}", g)));
    // 全埋点逐类断言。
    let inst_ok = {
        let mut ring = EventRing::new();
        let mut led = InstrumentLedger::new();
        let _ = emit_actions(&mut ring, &mut led, &DomainActions { change: 3, conflict: 2, fallback: 1 }, 1);
        led.all_instrumented(3, 2, 1)
    };
    v.push(("J1-全埋点逐类断言", inst_ok, String::from("变更3/冲突2/回退1 逐类对账")));
    // 注入三类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    v.push((
        "J1-注入三类10/10",
        inj.all_caught(JUDGE_ROUNDS) && inj.missed() == 0,
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    // 批量步长：10 条=3 批（4+4+2），单批≤4。
    let batch_ok = {
        let mut ring = EventRing::new();
        let mut led = InstrumentLedger::new();
        let _ = emit_actions(&mut ring, &mut led, &DomainActions { change: 6, conflict: 2, fallback: 2 }, 1);
        let _ = flush_all(&mut ring, &mut led);
        led.batches == 3 && led.flushed == 10
    };
    v.push(("J2-批量步长4/批", batch_ok, String::from("10 条=3 批")));
    // 非阻塞：满环丢弃入账（17 条 → 16 滞留 + 1 丢弃）。
    v.push(("J2-满环丢弃不阻塞", probe_seq_c().is_ok(), String::from("丢弃入账、push 仍 Ok")));
    // 空环落盘 no-op。
    v.push(("J2-空环落盘no-op", probe_seq_b().is_ok(), String::from("0 条 0 批")));
    // 守恒律（发出=落盘+滞留+丢弃）。
    let cons = {
        let mut ring = EventRing::new();
        let mut led = InstrumentLedger::new();
        let _ = emit_actions(&mut ring, &mut led, &DomainActions { change: 20, conflict: 0, fallback: 0 }, 1);
        let _ = flush_all(&mut ring, &mut led);
        led.conserve(ring.pending()).is_ok()
    };
    v.push(("J2-守恒律", cons, String::from("20 条全落盘账平")));
    // seq 单调（乱序/重复拒）。
    let seq_ok = {
        let mut ring = EventRing::new();
        let _ = ring.push(EventRec { seq: 1, kind: 0, code: 0, gen: 1 });
        let _ = ring.push(EventRec { seq: 2, kind: 0, code: 0, gen: 1 });
        ring.push(EventRec { seq: 1, kind: 0, code: 0, gen: 1 }).is_err()
    };
    v.push(("J2-seq单调闸", seq_ok, String::from("重复 seq 必拒")));
    // 探针三序。
    v.push(("J2-探针三序", probe_seq_a() == Ok(4u32), String::from("2+1+1 全落盘")));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫 + 文案闭集。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x80740 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("8 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18558_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18558 档案体验日志埋点（变更/冲突/回退全事件）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}
// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_full_instrument_and_batch() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, 10, "三类全埋点 5+3+2");
        assert_eq!(g.1, 10, "批量落盘 10 条");
        assert_eq!(g.2, 4, "溢出场景 4 条丢弃入账");
    }

    #[test]
    fn per_class_instrumentation() {
        let mut ring = EventRing::new();
        let mut led = InstrumentLedger::new();
        assert!(emit_actions(&mut ring, &mut led, &DomainActions { change: 4, conflict: 2, fallback: 1 }, 1).is_ok());
        assert!(led.all_instrumented(4, 2, 1), "逐类埋点计数==域动作数");
        assert_eq!(led.emitted_total(), 7);
    }

    #[test]
    fn batch_drain_without_blocking() {
        let mut ring = EventRing::new();
        let mut led = InstrumentLedger::new();
        let _ = emit_actions(&mut ring, &mut led, &DomainActions { change: 5, conflict: 3, fallback: 2 }, 1);
        let drained = flush_all(&mut ring, &mut led).unwrap_or(999);
        assert_eq!(drained, 10);
        assert_eq!(led.batches, 3, "4+4+2 = 3 批");
        assert_eq!(ring.pending(), 0);
        assert!(led.conserve(0).is_ok());
    }

    #[test]
    fn overflow_drops_accounted() {
        let mut ring = EventRing::new();
        let mut s = 1u32;
        let mut i = 0;
        while i < RING_CAP + 5 {
            let rec = EventRec { seq: s, kind: KIND_CONFLICT, code: 0, gen: 1 };
            let _ = ring.push(rec);
            s = s.saturating_add(1);
            i += 1;
        }
        assert_eq!(ring.dropped(), 5, "5 条丢弃入账");
        assert_eq!(ring.pending(), RING_CAP);
        assert!(ring.ring_ok());
    }

    #[test]
    fn injections_three_classes() {
        let inj = inject_round(JUDGE_ROUNDS);
        assert!(inj.all_caught(JUDGE_ROUNDS), "三类注入 10/10");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn seq_monotonic_gate() {
        let mut ring = EventRing::new();
        assert!(ring.push(EventRec { seq: 1, kind: 0, code: 0, gen: 1 }).is_ok());
        assert!(ring.push(EventRec { seq: 2, kind: 0, code: 0, gen: 1 }).is_ok());
        assert_eq!(
            ring.push(EventRec { seq: 2, kind: 0, code: 0, gen: 1 }),
            Err(ElCode::SeqDup.code())
        );
        assert_eq!(
            ring.push(EventRec { seq: 1, kind: 0, code: 0, gen: 1 }),
            Err(ElCode::SeqRegress.code())
        );
    }

    #[test]
    fn empty_ring_and_repro() {
        assert_eq!(probe_seq_b(), Ok(()), "空环落盘 no-op");
        assert!(repro_check().is_ok(), "双跑一致");
        assert!(segment_guard());
        assert!(detail_closed());
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18558_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
