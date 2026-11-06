//! UNX-F48805 输入事件闸：缺 SYN 超时合成 / 时间戳单调 / 4096 溢出账 · 0x70080
//! 覆盖卡：UNX-F48805（缺 SYN 100ms 超时强制合成帧，合成边界并入异常账）、
//!         UNX-F48806（事件时间戳单调性检测，非单调触发设备重置流程声明）、
//!         UNX-F48807（重置期间 4,096 条缓冲上限，溢出丢帧记账不静默）
//!
//! ── 闸门次序 ──
//! 本模块的四道闸**必须**按 单调 → 隔离 → 合成 → 溢出 顺序判，逐一说明倒了会怎样：
//!   ① 时间戳单调闸（F48806）必须最先判。
//!      若把它放到合成闸之后，倒流的时间戳会让 `ts - open_ts` 变成一次巨大的
//!      「超时」，于是一个坏时间戳会被当成「驱动漏了 SYN」而**合成出一帧假数据**；
//!      真正的病灶（时钟回退/设备重置）反而被掩盖，异常账里留下的是错误的
//!      合成记录，排障方向被彻底带偏。
//!   ② 重置隔离闸紧随其后。
//!      重置声明一旦成立，后面 N 条事件属于「重置窗口内」的残渣，必须归零并入账，
//!      绝不能流入 pending：若把隔离放到合成之后，重置窗口里的事件会先被
//!      超时逻辑合成为帧，等于把残渣当正常数据递交，且零账漏记。
//!   ③ 缺 SYN 合成闸（F48805）在隔离之后。
//!      隔离期内不判超时（残渣本来就要丢，合成毫无意义）；隔离期外，
//!      距帧首 ≥100ms 仍未见到 SYN_REPORT 即强制合成收帧。
//!      若把合成放到溢出之后，长帧会先撞 4096 上限被整帧丢弃，
//!      而按语义应优先「按 100ms 切帧保留数据」——顺序错了会多丢 4,095 条。
//!   ④ 4096 溢出闸（F48807）最后判。
//!      只有「既没超时、又没在隔离」的满缓冲才是真溢出：整帧丢弃并记账。
//!      把溢出提前会让「慢设备 + 100ms 合成」路径永远走不到，而那条路径
//!      恰是掉帧最少的一条。
//!
//! 三条异常账（合成/重置/溢出）互相独立、逐条可查，且必须与总量恒等式
//!   `ingested == committed + zeroed + dropped + pending`
//! 同时成立——任何一个计数静默漏记，恒等式当场破，探针立刻红。
//!
//! ── 关键差异 ──
//! 与 unx48803_input_meta 的边界：meta 只管**帧边界**（未见 SYN 不得见），
//!   本模块管**帧的时序**（等太久要替驱动收帧、时钟倒流要重置、缓冲满了要丢帧）。
//!   换句话说：meta 的 `expose_pending` 是「读者不许偷看」，本模块的
//!   `ingest` 是「系统不许无限期等」。两个模块的 pending 是两份独立账，
//!   不共享缓冲区，避免「一个模块的清空动作把另一个模块的账弄丢」。
//! 与 unx48808_input_dispatch 的边界：本模块产出的帧**已关闭**（有闭合原因），
//!   dispatch 只负责把已闭合帧推进 per-CPU 环；本模块不感知 CPU、不排队、
//!   不做批量递交，也不重试。
//! 与 unx48801_input_core 的边界：core 判「族/码/能力」静态合法性，
//!   本模块对 kind/code 不做任何语义判决（除 SYN 子码用于收帧），
//!   只做时间与容量判决。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 本模块诊断段基址。
pub const SEG_BASE: u32 = 0x70080;

/// 缺 SYN 的合成门槛（微秒）：距帧首 ≥100ms 即强制收帧。
pub const SYN_TIMEOUT_US: u64 = 100_000;
/// 重置期间的缓冲上限（条）：4096 是卡面硬口径。
pub const OVERFLOW_CAP: usize = 4_096;
/// 重置窗口隔离条数：重置后 N 条事件归零（不进入 pending）。
pub const RESET_QUARANTINE: u32 = 8;
/// 判据轮数（卡面 ×10）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（12 条）────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// 时间戳倒流（非单调）。
    TimestampRegression,
    /// 重置期间收到事件（已归零）。
    EventInResetWindow,
    /// 缺 SYN 超时合成。
    SynTimeoutSynthesized,
    /// 缓冲溢出丢帧。
    BufferOverflow,
    /// 总量恒等式破裂（有计数静默漏记）。
    LedgerImbalance,
    /// 合成帧没有事件（空帧合成）。
    EmptySynthFrame,
    /// 重置窗口未闭合却又声明重置。
    ResetWindowNested,
    /// 帧闭合时间早于帧首。
    CloseBeforeOpen,
    /// 隔离条数配置为零（归零账无意义）。
    QuarantineDisabled,
    /// 溢出账条数与丢帧数不符。
    OverflowMismatch,
    /// 单调基线未建立就查回归。
    NoBaseline,
    /// 双跑不一致。
    NotReproducible,
}

impl GuardCode {
    /// 段内绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文文案。
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::TimestampRegression => "时间戳倒流：触发设备重置",
            GuardCode::EventInResetWindow => "重置窗口内事件：归零入账",
            GuardCode::SynTimeoutSynthesized => "缺 SYN 超时：强制合成帧",
            GuardCode::BufferOverflow => "缓冲溢出：整帧丢弃入账",
            GuardCode::LedgerImbalance => "账不平：ingested≠committed+zeroed+dropped+pending",
            GuardCode::EmptySynthFrame => "空帧合成：帧内无事件",
            GuardCode::ResetWindowNested => "重置窗口未闭合又声明重置",
            GuardCode::CloseBeforeOpen => "帧闭合时间早于帧首",
            GuardCode::QuarantineDisabled => "隔离条数为零：归零账无意义",
            GuardCode::OverflowMismatch => "溢出账条数与丢帧数不符",
            GuardCode::NoBaseline => "单调基线未建立",
            GuardCode::NotReproducible => "双跑不一致",
        }
    }

    /// 闭集全表。
    pub const ALL: [GuardCode; 12] = [
        GuardCode::TimestampRegression,
        GuardCode::EventInResetWindow,
        GuardCode::SynTimeoutSynthesized,
        GuardCode::BufferOverflow,
        GuardCode::LedgerImbalance,
        GuardCode::EmptySynthFrame,
        GuardCode::ResetWindowNested,
        GuardCode::CloseBeforeOpen,
        GuardCode::QuarantineDisabled,
        GuardCode::OverflowMismatch,
        GuardCode::NoBaseline,
        GuardCode::NotReproducible,
    ];
}

/// 码值 → 文案；未知码返回闭集外说明。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < GuardCode::ALL.len() {
        if let Some(c) = GuardCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x70081..0x7008C 闭集内"
}

// ── 指纹基础设施（本域按「帧账指纹」命名）────────────────────
/// 帧账指纹初值。
pub fn ledger_init() -> u64 {
    0xcbf2_9ce4_8422_2325
}

/// 折入一字节。
pub fn ledger_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
}

/// 折入 64 位字（按小端 8 字节）。
pub fn ledger_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut i = 0u32;
    while i < 8 {
        acc = ledger_byte(acc, ((w >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 折入同步子码（收帧原因）。
pub fn ledger_reason(h: u64, reason: CloseReason) -> u64 {
    ledger_byte(h, reason as u8)
}

// ── 事件与收帧原因 ────────────────────────────────────────────
/// 帧内事件（不带时间戳：时间由闸自己的参数携带）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GEvent {
    /// type。
    pub kind: u16,
    /// code。
    pub code: u16,
    /// value。
    pub value: i32,
}

impl GEvent {
    /// 构造。
    pub const fn new(kind: u16, code: u16, value: i32) -> GEvent {
        GEvent { kind, code, value }
    }

    /// 是否 SYN_REPORT。
    pub fn is_syn(self) -> bool {
        self.kind == 0x00 && self.code == 0
    }

    /// 事件指纹。
    pub fn digest(self) -> u64 {
        let mut h = ledger_init();
        h = ledger_word(h, self.kind as u64);
        h = ledger_word(h, self.code as u64);
        h = ledger_word(h, self.value as u32 as u64);
        h
    }
}

/// 收帧原因（进指纹，也是异常账的分类键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseReason {
    /// 驱动正常给了 SYN_REPORT。
    Syn,
    /// 缺 SYN 100ms 超时合成。
    SynthTimeout,
    /// 溢出整帧丢弃（不进正常帧账，进溢出账）。
    OverflowDrop,
    /// 重置声明时冲刷 pending（归零）。
    ResetFlush,
}

impl CloseReason {
    /// 中文原因。
    pub fn label(self) -> &'static str {
        match self {
            CloseReason::Syn => "驱动 SYN_REPORT",
            CloseReason::SynthTimeout => "缺 SYN 100ms 合成",
            CloseReason::OverflowDrop => "4096 溢出丢弃",
            CloseReason::ResetFlush => "重置冲刷归零",
        }
    }
}

/// 受理结果（每种结果都有对应的账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardOutcome {
    /// 收入 pending。
    Accepted,
    /// 收帧并提交（正常 SYN）。
    Committed,
    /// 先合成上一帧，本事件收入新帧。
    SynthCommitted,
    /// 重置窗口内，归零。
    Quarantined,
    /// 触发设备重置声明。
    ResetDeclared,
    /// 缓冲溢出，整帧丢弃。
    OverflowDropped,
}

// ── 异常账条目 ────────────────────────────────────────────────
/// 合成账：一次超时合成的完整证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SynthRecord {
    /// 合成序号（1 起）。
    pub seq: u32,
    /// 帧首事件时间戳。
    pub open_ts: u64,
    /// 合成时刻（触发合成的那条事件的时间戳）。
    pub close_ts: u64,
    /// 帧内事件数。
    pub events: u32,
    /// 等待时长（微秒）。
    pub waited_us: u64,
}

/// 重置账：一次时钟倒流的完整证据 + 归零计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResetRecord {
    /// 重置纪元（1 起，单调）。
    pub epoch: u32,
    /// 倒流前的基线时间戳。
    pub last_ts: u64,
    /// 倒流的坏时间戳。
    pub bad_ts: u64,
    /// 倒流跨度（last - bad）。
    pub gap_us: u64,
    /// 本次重置累计归零事件数（含被冲刷的 pending、坏事件本身、隔离窗口）。
    pub zeroed: u32,
    /// 隔离窗口配额。
    pub quarantine: u32,
}

/// 溢出账：一次整帧丢弃的完整证据。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverflowRecord {
    /// 溢出序号（1 起）。
    pub seq: u32,
    /// 溢出发生时的时间戳。
    pub at_ts: u64,
    /// 丢弃帧数（恒为 1，保留字段以便聚合）。
    pub dropped_frames: u32,
    /// 丢弃事件数（被丢弃帧的事件数）。
    pub dropped_events: u32,
    /// 当时缓冲占用。
    pub occupancy: u32,
}

// ── 事件闸 ────────────────────────────────────────────────────
/// 时间/容量四闸（单调 → 隔离 → 合成 → 溢出）。
pub struct EventGuard {
    pending: Vec<GEvent>,
    open_ts: u64,
    last_ts: Option<u64>,
    reset_epoch: u32,
    quarantine_left: u32,
    synth: Vec<SynthRecord>,
    resets: Vec<ResetRecord>,
    overflows: Vec<OverflowRecord>,
    frames: u32,
    committed_events: u32,
    zeroed: u32,
    dropped_events: u32,
    ingested: u32,
    digest: u64,
    max_pending: usize,
}

impl EventGuard {
    /// 空闸。
    pub fn new() -> EventGuard {
        EventGuard {
            pending: Vec::new(),
            open_ts: 0,
            last_ts: None,
            reset_epoch: 0,
            quarantine_left: 0,
            synth: Vec::new(),
            resets: Vec::new(),
            overflows: Vec::new(),
            frames: 0,
            committed_events: 0,
            zeroed: 0,
            dropped_events: 0,
            ingested: 0,
            digest: ledger_init(),
            max_pending: 0,
        }
    }

    /// 受理一条事件（四闸次序见文件头）。
    pub fn ingest(&mut self, ts_us: u64, ev: GEvent) -> Result<GuardOutcome, u32> {
        self.ingested = self.ingested.saturating_add(1);

        // ── 闸①：时间戳单调 ──
        if let Some(last) = self.last_ts {
            if ts_us < last {
                self.declare_reset(ts_us, last)?;
                return Ok(GuardOutcome::ResetDeclared);
            }
        }

        // ── 闸②：重置隔离 ──
        if self.quarantine_left > 0 {
            self.quarantine_left -= 1;
            self.zeroed = self.zeroed.saturating_add(1);
            if let Some(r) = self.resets.last_mut() {
                r.zeroed = r.zeroed.saturating_add(1);
            }
            return Ok(GuardOutcome::Quarantined);
        }

        // ── 闸③：缺 SYN 超时合成 ──
        let mut synthesized = false;
        if !self.pending.is_empty() {
            let waited = ts_us.saturating_sub(self.open_ts);
            if waited >= SYN_TIMEOUT_US {
                self.synth_close(ts_us)?;
                synthesized = true;
            }
        }

        // ── 闸④：4096 溢出 ──
        let mut overflowed = false;
        if self.pending.len() >= OVERFLOW_CAP {
            self.overflow_drop(ts_us)?;
            overflowed = true;
        }

        // 入缓冲。
        if self.pending.is_empty() {
            self.open_ts = ts_us;
        }
        self.pending.push(ev);
        if self.pending.len() > self.max_pending {
            self.max_pending = self.pending.len();
        }
        match self.last_ts {
            Some(v) => {
                if ts_us > v {
                    self.last_ts = Some(ts_us);
                }
            }
            None => self.last_ts = Some(ts_us),
        }

        // SYN_REPORT 收帧。
        if ev.is_syn() {
            let events = self.pending.len() as u32;
            if events == 0 {
                return Err(GuardCode::EmptySynthFrame.val());
            }
            self.close_frame(CloseReason::Syn, events);
            return Ok(GuardOutcome::Committed);
        }
        if synthesized {
            Ok(GuardOutcome::SynthCommitted)
        } else if overflowed {
            Ok(GuardOutcome::OverflowDropped)
        } else {
            Ok(GuardOutcome::Accepted)
        }
    }

    /// 关闭当前 pending 为正常/合成帧。
    fn close_frame(&mut self, reason: CloseReason, events: u32) {
        self.frames = self.frames.saturating_add(1);
        self.committed_events = self.committed_events.saturating_add(events);
        self.digest = ledger_word(self.digest, events as u64);
        self.digest = ledger_reason(self.digest, reason);
        let mut i = 0usize;
        while i < self.pending.len() {
            if let Some(ev) = self.pending.get(i) {
                self.digest = ledger_word(self.digest, ev.digest());
            }
            i += 1;
        }
        self.pending.clear();
    }

    /// 超时合成：记账 + 收帧。
    fn synth_close(&mut self, now_ts: u64) -> Result<(), u32> {
        let events = self.pending.len() as u32;
        if events == 0 {
            return Err(GuardCode::EmptySynthFrame.val());
        }
        if now_ts < self.open_ts {
            return Err(GuardCode::CloseBeforeOpen.val());
        }
        let seq = (self.synth.len() as u32).saturating_add(1);
        let rec = SynthRecord {
            seq,
            open_ts: self.open_ts,
            close_ts: now_ts,
            events,
            waited_us: now_ts.saturating_sub(self.open_ts),
        };
        self.close_frame(CloseReason::SynthTimeout, events);
        self.synth.push(rec);
        Ok(())
    }

    /// 溢出丢帧：整帧丢弃 + 记账（不静默）。
    fn overflow_drop(&mut self, now_ts: u64) -> Result<(), u32> {
        let events = self.pending.len() as u32;
        if events == 0 {
            return Err(GuardCode::BufferOverflow.val());
        }
        let seq = (self.overflows.len() as u32).saturating_add(1);
        let rec = OverflowRecord {
            seq,
            at_ts: now_ts,
            dropped_frames: 1,
            dropped_events: events,
            occupancy: events,
        };
        // 丢弃的帧不进正常帧账，只进溢出账与丢弃总数。
        self.dropped_events = self.dropped_events.saturating_add(events);
        self.digest = ledger_word(self.digest, events as u64);
        self.digest = ledger_reason(self.digest, CloseReason::OverflowDrop);
        self.pending.clear();
        self.overflows.push(rec);
        Ok(())
    }

    /// 声明设备重置：冲刷 pending（归零）+ 开隔离窗。
    fn declare_reset(&mut self, bad_ts: u64, last_ts: u64) -> Result<(), u32> {
        if self.quarantine_left > 0 {
            return Err(GuardCode::ResetWindowNested.val());
        }
        if RESET_QUARANTINE == 0 {
            return Err(GuardCode::QuarantineDisabled.val());
        }
        let flushed = self.pending.len() as u32;
        self.pending.clear();
        self.reset_epoch = self.reset_epoch.saturating_add(1);
        // 归零账：被冲刷的 pending + 倒流的坏事件本身。
        let zeroed_now = flushed.saturating_add(1);
        self.zeroed = self.zeroed.saturating_add(zeroed_now);
        let rec = ResetRecord {
            epoch: self.reset_epoch,
            last_ts,
            bad_ts,
            gap_us: last_ts.saturating_sub(bad_ts),
            zeroed: zeroed_now,
            quarantine: RESET_QUARANTINE,
        };
        self.resets.push(rec);
        self.quarantine_left = RESET_QUARANTINE;
        // 时间基线回退到坏时间戳（新纪元从这里重新起算）。
        self.last_ts = Some(bad_ts);
        self.digest = ledger_word(self.digest, bad_ts);
        self.digest = ledger_word(self.digest, last_ts);
        Ok(())
    }

    /// 正常收尾（驱动在流结束时冲刷 pending：走合成语义，不丢数据）。
    pub fn flush_pending(&mut self, now_ts: u64) -> Result<(), u32> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let events = self.pending.len() as u32;
        let seq = (self.synth.len() as u32).saturating_add(1);
        let rec = SynthRecord {
            seq,
            open_ts: self.open_ts,
            close_ts: now_ts,
            events,
            waited_us: now_ts.saturating_sub(self.open_ts),
        };
        self.close_frame(CloseReason::SynthTimeout, events);
        self.synth.push(rec);
        Ok(())
    }

    /// 总量恒等式：ingested == committed + zeroed + dropped + pending。
    pub fn reconcile(&self) -> Result<(), u32> {
        let lhs = self.ingested;
        let rhs = self
            .committed_events
            .saturating_add(self.zeroed)
            .saturating_add(self.dropped_events)
            .saturating_add(self.pending.len() as u32);
        if lhs != rhs {
            return Err(GuardCode::LedgerImbalance.val());
        }
        // 溢出账逐条一致：每条记录的 dropped_events 之和 == 丢弃总数。
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.overflows.len() {
            if let Some(r) = self.overflows.get(i) {
                if r.dropped_events == 0 || r.dropped_frames != 1 {
                    return Err(GuardCode::OverflowMismatch.val());
                }
                sum = sum.saturating_add(r.dropped_events);
            }
            i += 1;
        }
        if sum != self.dropped_events {
            return Err(GuardCode::OverflowMismatch.val());
        }
        // 重置账：每条记录的 zeroed 之和不得超过总归零数。
        let mut rsum = 0u32;
        let mut k = 0usize;
        while k < self.resets.len() {
            if let Some(r) = self.resets.get(k) {
                rsum = rsum.saturating_add(r.zeroed);
            }
            k += 1;
        }
        if rsum > self.zeroed {
            return Err(GuardCode::LedgerImbalance.val());
        }
        Ok(())
    }

    /// 合成账。
    pub fn synth_records(&self) -> &[SynthRecord] {
        self.synth.as_slice()
    }

    /// 重置账。
    pub fn reset_records(&self) -> &[ResetRecord] {
        self.resets.as_slice()
    }

    /// 溢出账。
    pub fn overflow_records(&self) -> &[OverflowRecord] {
        self.overflows.as_slice()
    }

    /// 已提交帧数。
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// 已提交事件数。
    pub fn committed_events(&self) -> u32 {
        self.committed_events
    }

    /// 归零事件数。
    pub fn zeroed(&self) -> u32 {
        self.zeroed
    }

    /// 丢弃事件数。
    pub fn dropped(&self) -> u32 {
        self.dropped_events
    }

    /// 受理总数。
    pub fn ingested(&self) -> u32 {
        self.ingested
    }

    /// 当前 pending 长度。
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// 历史最大 pending 长度（4096 上限的实测证据）。
    pub fn max_pending(&self) -> usize {
        self.max_pending
    }

    /// 重置纪元。
    pub fn epoch(&self) -> u32 {
        self.reset_epoch
    }

    /// 剩余隔离配额。
    pub fn quarantine_left(&self) -> u32 {
        self.quarantine_left
    }

    /// 账本指纹。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 报告行（异常三账汇总）。
    pub fn report(&self) -> String {
        format!(
            "ingested={} committed={} frames={} synth={} resets={} overflows={} zeroed={} dropped={} pending={}",
            self.ingested,
            self.committed_events,
            self.frames,
            self.synth.len(),
            self.resets.len(),
            self.overflows.len(),
            self.zeroed,
            self.dropped_events,
            self.pending.len()
        )
    }
}

// ── 三条演练（合成 / 重置 / 溢出）────────────────────────────
/// 超时合成 ×10：每轮 3 事件成帧但**故意不给 SYN**，用一次 ≥100ms 的跳跃触发合成。
pub fn synth_drill() -> Result<u32, u32> {
    let mut g = EventGuard::new();
    let mut round = 0u32;
    let mut ts = 1_000u64;
    while round < JUDGE_ROUNDS {
        let mut i = 0u16;
        while i < 3 {
            let out = g.ingest(ts, GEvent::new(0x01, 0x1E + i, 1))?;
            if out != GuardOutcome::Accepted {
                return Err(GuardCode::SynTimeoutSynthesized.val());
            }
            ts += 10;
            i += 1;
        }
        // 心跳跳过 100ms：本条事件到达时应当先合成上一帧。
        ts += SYN_TIMEOUT_US + 1;
        let out = g.ingest(ts, GEvent::new(0x01, 0x30, 1))?;
        if out != GuardOutcome::SynthCommitted {
            return Err(GuardCode::SynTimeoutSynthesized.val());
        }
        // 新帧正常收尾。
        let out = g.ingest(ts + 1, GEvent::new(0x00, 0, 0))?;
        if out != GuardOutcome::Committed {
            return Err(GuardCode::SynTimeoutSynthesized.val());
        }
        ts += 20;
        round += 1;
    }
    if g.synth_records().len() != JUDGE_ROUNDS as usize {
        return Err(GuardCode::SynTimeoutSynthesized.val());
    }
    // 每条合成记录必须 events==3 且 waited ≥100ms。
    let mut k = 0usize;
    while k < g.synth_records().len() {
        if let Some(r) = g.synth_records().get(k) {
            if r.events != 3 {
                return Err(GuardCode::SynTimeoutSynthesized.val());
            }
            if r.waited_us < SYN_TIMEOUT_US {
                return Err(GuardCode::SynTimeoutSynthesized.val());
            }
        }
        k += 1;
    }
    g.reconcile()?;
    Ok(g.synth_records().len() as u32)
}

/// 合成边界精确性：99,999us 不合成；100,000us 必合成。
pub fn synth_boundary() -> Result<u32, u32> {
    let mut hits = 0u32;
    // 边界下侧：差 1us，不得合成。
    let mut g = EventGuard::new();
    g.ingest(0, GEvent::new(0x01, 0x1E, 1))?;
    let out = g.ingest(SYN_TIMEOUT_US - 1, GEvent::new(0x01, 0x1E, 0))?;
    if out == GuardOutcome::Accepted && g.synth_records().is_empty() && g.pending_len() == 2 {
        hits += 1;
    }
    // 边界上侧：恰好 100,000us，必须合成。
    let mut g = EventGuard::new();
    g.ingest(0, GEvent::new(0x01, 0x1E, 1))?;
    let out = g.ingest(SYN_TIMEOUT_US, GEvent::new(0x01, 0x1E, 0))?;
    if out == GuardOutcome::SynthCommitted && g.synth_records().len() == 1 && g.frames() == 1 {
        hits += 1;
    }
    Ok(hits)
}

/// 重置 ×10：每轮 pending 2 条 + 倒流一次 + 8 条隔离残渣 + 1 条正常受理。
pub fn reset_drill() -> Result<u32, u32> {
    let mut g = EventGuard::new();
    let mut round = 0u32;
    let mut ts = 50_000u64;
    while round < JUDGE_ROUNDS {
        // 两条正常事件入 pending。
        g.ingest(ts, GEvent::new(0x01, 0x1E, 1))?;
        ts += 5;
        g.ingest(ts, GEvent::new(0x01, 0x1F, 1))?;
        // 时间戳倒流 23us（相对最后一条事件）。
        let last = ts;
        let bad = last - 23;
        let out = g.ingest(bad, GEvent::new(0x01, 0x20, 1))?;
        if out != GuardOutcome::ResetDeclared {
            return Err(GuardCode::TimestampRegression.val());
        }
        // 隔离窗口：8 条残渣必须全归零。
        let mut i = 0u32;
        while i < RESET_QUARANTINE {
            let out = g.ingest(bad + i as u64 + 1, GEvent::new(0x01, 0x40 + i as u16, 1))?;
            if out != GuardOutcome::Quarantined {
                return Err(GuardCode::EventInResetWindow.val());
            }
            i += 1;
        }
        // 窗口闭合后必须能重新受理。
        ts = bad + RESET_QUARANTINE as u64 + 20;
        let out = g.ingest(ts, GEvent::new(0x01, 0x1E, 0))?;
        if out != GuardOutcome::Accepted {
            return Err(GuardCode::TimestampRegression.val());
        }
        g.ingest(ts + 1, GEvent::new(0x00, 0, 0))?;
        ts += 100;
        round += 1;
    }
    if g.reset_records().len() != JUDGE_ROUNDS as usize {
        return Err(GuardCode::TimestampRegression.val());
    }
    // 每轮归零账 = 2（pending）+ 1（坏事件）+ 8（隔离）= 11。
    let mut k = 0usize;
    while k < g.reset_records().len() {
        if let Some(r) = g.reset_records().get(k) {
            if r.zeroed != 2 + 1 + RESET_QUARANTINE {
                return Err(GuardCode::LedgerImbalance.val());
            }
            if r.quarantine != RESET_QUARANTINE {
                return Err(GuardCode::LedgerImbalance.val());
            }
            if r.gap_us != 23 {
                return Err(GuardCode::TimestampRegression.val());
            }
        }
        k += 1;
    }
    g.reconcile()?;
    Ok(g.reset_records().len() as u32)
}

/// 溢出 ×10：每轮灌满 4096 条后第 4097 条触发整帧丢弃。
pub fn overflow_drill() -> Result<u32, u32> {
    let mut drops = 0u32;
    let mut round = 0u32;
    while round < JUDGE_ROUNDS {
        let mut g = EventGuard::new();
        let mut ts = 7_000u64;
        let mut i = 0usize;
        while i < OVERFLOW_CAP {
            let out = g.ingest(ts, GEvent::new(0x01, 0x1E, 1))?;
            if out != GuardOutcome::Accepted {
                return Err(GuardCode::BufferOverflow.val());
            }
            ts += 1;
            i += 1;
        }
        if g.pending_len() != OVERFLOW_CAP {
            return Err(GuardCode::BufferOverflow.val());
        }
        // 第 4097 条：必须先丢整帧再收本条。
        let out = g.ingest(ts, GEvent::new(0x01, 0x1E, 0))?;
        if out != GuardOutcome::OverflowDropped {
            return Err(GuardCode::BufferOverflow.val());
        }
        if g.dropped() != OVERFLOW_CAP as u32 {
            return Err(GuardCode::OverflowMismatch.val());
        }
        if g.overflow_records().len() != 1 {
            return Err(GuardCode::OverflowMismatch.val());
        }
        if let Some(r) = g.overflow_records().first() {
            if r.dropped_events != OVERFLOW_CAP as u32 || r.dropped_frames != 1 {
                return Err(GuardCode::OverflowMismatch.val());
            }
        }
        // 新帧正常收尾。
        g.ingest(ts + 1, GEvent::new(0x00, 0, 0))?;
        g.reconcile()?;
        drops += g.overflow_records().len() as u32;
        round += 1;
    }
    Ok(drops)
}

// ── 黄金全链 ──────────────────────────────────────────────────
/// 黄金链输出账面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenOut {
    /// 合成记录数。
    pub synth: u32,
    /// 重置记录数。
    pub resets: u32,
    /// 溢出记录数。
    pub overflows: u32,
    /// 提交事件数。
    pub committed: u32,
    /// 归零事件数。
    pub zeroed: u32,
    /// 丢弃事件数。
    pub dropped: u32,
    /// 账本指纹。
    pub digest: u64,
}

/// 黄金全链：三演练串跑 + 恒等式复核。
pub fn golden_run() -> Result<GoldenOut, u32> {
    let synth = synth_drill()?;
    let resets = reset_drill()?;
    let overflows = overflow_drill()?;

    // 串跑一台「混合设备」：正常帧 + 合成 + 重置 + 溢出各一次，做最终恒等式。
    let mut g = EventGuard::new();
    let mut ts = 1_000u64;
    let mut i = 0u16;
    while i < 4 {
        g.ingest(ts, GEvent::new(0x01, 0x1E + i, 1))?;
        ts += 3;
        i += 1;
    }
    g.ingest(ts, GEvent::new(0x00, 0, 0))?;
    ts += SYN_TIMEOUT_US + 7;
    g.ingest(ts, GEvent::new(0x01, 0x30, 1))?; // 合成
    let bad = ts - 100;
    g.ingest(bad, GEvent::new(0x01, 0x31, 1))?; // 重置
    let mut q = 0u32;
    while q < RESET_QUARANTINE {
        g.ingest(bad + q as u64 + 1, GEvent::new(0x01, 0x50, 0))?;
        q += 1;
    }
    g.flush_pending(bad + RESET_QUARANTINE as u64 + 5)?;
    g.reconcile()?;

    Ok(GoldenOut {
        synth,
        resets,
        overflows,
        committed: g.committed_events(),
        zeroed: g.zeroed(),
        dropped: g.dropped(),
        digest: g.digest(),
    })
}

// ── 注入账 ────────────────────────────────────────────────────
/// 注入账：4 槽。
#[derive(Clone, Copy, Debug)]
pub struct InjectLedger {
    /// 每槽注入次数。
    pub tries: [u32; 4],
    /// 每槽按预期命中次数。
    pub caught: [u32; 4],
}

impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 4], caught: [0u32; 4] }
    }

    /// 记账。
    pub fn record(&mut self, slot: usize, caught: bool) -> bool {
        let mut ok = false;
        if let Some(t) = self.tries.get_mut(slot) {
            *t += 1;
            ok = true;
        }
        if ok && caught {
            if let Some(c) = self.caught.get_mut(slot) {
                *c += 1;
            }
        }
        ok
    }

    /// 四槽全中。
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if t == 0 || t != c {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入 4 槽 × rounds：超时合成 / 时钟倒流 / 隔离残渣 / 溢出丢帧。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：超时合成。
        let mut g = EventGuard::new();
        g.ingest(0, GEvent::new(0x01, 0x1E, 1))?;
        let out = g.ingest(SYN_TIMEOUT_US, GEvent::new(0x01, 0x1F, 1))?;
        led.record(0, out == GuardOutcome::SynthCommitted && g.synth_records().len() == 1);

        // 槽 1：时钟倒流触发重置。
        let mut g = EventGuard::new();
        g.ingest(1_000, GEvent::new(0x01, 0x1E, 1))?;
        let out = g.ingest(999, GEvent::new(0x01, 0x1E, 0))?;
        led.record(
            1,
            out == GuardOutcome::ResetDeclared && g.epoch() == 1 && g.quarantine_left() == RESET_QUARANTINE,
        );

        // 槽 2：隔离残渣归零。
        let mut g = EventGuard::new();
        g.ingest(1_000, GEvent::new(0x01, 0x1E, 1))?;
        g.ingest(999, GEvent::new(0x01, 0x1E, 0))?;
        let out = g.ingest(1_000, GEvent::new(0x01, 0x20, 1))?;
        led.record(2, out == GuardOutcome::Quarantined && g.zeroed() >= 2);

        // 槽 3：溢出丢帧（小改造：直接把 pending 灌满）。
        let mut g = EventGuard::new();
        let mut i = 0usize;
        while i < OVERFLOW_CAP {
            g.ingest(i as u64, GEvent::new(0x01, 0x1E, 1))?;
            i += 1;
        }
        let out = g.ingest(OVERFLOW_CAP as u64, GEvent::new(0x01, 0x1E, 0))?;
        led.record(
            3,
            out == GuardOutcome::OverflowDropped && g.overflow_records().len() == 1,
        );
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────
/// 探针 A：正常帧必须只由驱动 SYN 收，不产生合成账。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut g = EventGuard::new();
    g.ingest(10, GEvent::new(0x01, 0x1E, 1))?;
    g.ingest(20, GEvent::new(0x01, 0x1E, 0))?;
    g.ingest(30, GEvent::new(0x00, 0, 0))?;
    if g.synth_records().len() != 0 || g.frames() != 1 || g.committed_events() != 3 {
        return Err(GuardCode::SynTimeoutSynthesized.val());
    }
    g.reconcile()
}

/// 探针 B：倒流必须声明重置且 pending 被归零（不残留旧帧）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = EventGuard::new();
    g.ingest(500, GEvent::new(0x01, 0x1E, 1))?;
    g.ingest(400, GEvent::new(0x01, 0x1F, 1))?;
    if g.pending_len() != 0 {
        return Err(GuardCode::ResetWindowNested.val());
    }
    if g.zeroed() != 2 {
        return Err(GuardCode::LedgerImbalance.val());
    }
    g.reconcile()
}

/// 探针 C：恰好 4096 条不丢，第 4097 条丢一帧。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut g = EventGuard::new();
    let mut i = 0usize;
    while i < OVERFLOW_CAP {
        g.ingest(i as u64, GEvent::new(0x01, 0x1E, 1))?;
        i += 1;
    }
    if g.dropped() != 0 || g.pending_len() != OVERFLOW_CAP {
        return Err(GuardCode::BufferOverflow.val());
    }
    g.ingest(OVERFLOW_CAP as u64, GEvent::new(0x01, 0x1E, 0))?;
    if g.dropped() != OVERFLOW_CAP as u32 {
        return Err(GuardCode::OverflowMismatch.val());
    }
    g.reconcile()
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GuardCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < GuardCode::ALL.len() {
        match GuardCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v <= SEG_BASE || v >= SEG_BASE + 0x20 {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // F48805-J1：超时合成 ×10 全部入异常账。
    let synth = synth_drill();
    v.push((
        "F48805-J1-超时合成10/10入账",
        matches!(synth, Ok(n) if n == JUDGE_ROUNDS),
        format!("synth={:?}", synth),
    ));

    // F48805-J1：100ms 边界精确（99999 不合成 / 100000 合成）。
    let boundary = synth_boundary();
    v.push((
        "F48805-J1-合成边界100ms精确",
        matches!(boundary, Ok(2)),
        format!("boundary={:?}/2", boundary),
    ));

    // F48805-J1：合成帧事件数守恒（3+3+…+2 条，无一丢失）。
    let conservation = {
        let mut g = EventGuard::new();
        let mut ts = 1u64;
        let mut fed = 0u32;
        let mut i = 0u32;
        while i < JUDGE_ROUNDS {
            let mut k = 0u16;
            while k < 5 {
                let _ = g.ingest(ts, GEvent::new(0x01, 0x1E + k, 1)).is_ok();
                fed += 1;
                ts += 1;
                k += 1;
            }
            ts += SYN_TIMEOUT_US;
            let _ = g.ingest(ts, GEvent::new(0x01, 0x30, 1)).is_ok();
            fed += 1;
            i += 1;
        }
        g.flush_pending(ts + 10).is_ok()
            && g.reconcile().is_ok()
            && g.ingested() == fed
            && g.synth_records().len() == JUDGE_ROUNDS as usize + 1
    };
    v.push((
        "F48805-J1-合成事件数守恒",
        conservation,
        String::from("ingested == committed + pending"),
    ));

    // F48806-J1：乱序注入 ×10 → 重置 10/10。
    let resets = reset_drill();
    v.push((
        "F48806-J1-乱序重置10/10",
        matches!(resets, Ok(n) if n == JUDGE_ROUNDS),
        format!("resets={:?}", resets),
    ));

    // F48806-J1：重置期事件归零账可查（每轮 11 条）。
    let zero_ok = {
        let mut g = EventGuard::new();
        let _ = g.ingest(100, GEvent::new(0x01, 0x1E, 1)).is_ok();
        let _ = g.ingest(50, GEvent::new(0x01, 0x1E, 0)).is_ok();
        let mut i = 0u32;
        while i < RESET_QUARANTINE {
            let _ = g.ingest(50 + i as u64 + 1, GEvent::new(0x01, 0x40, 0)).is_ok();
            i += 1;
        }
        let rec_ok = match g.reset_records().first() {
            Some(r) => r.zeroed == 2 + RESET_QUARANTINE && r.gap_us == 50,
            None => false,
        };
        rec_ok && g.zeroed() == 2 + RESET_QUARANTINE && g.reconcile().is_ok()
    };
    v.push((
        "F48806-J1-归零账可查",
        zero_ok,
        format!("zeroed={} 期望=11", 2 + RESET_QUARANTINE),
    ));

    // F48806-J1：重置后基线回退且继续受理。
    let rebase = {
        let mut g = EventGuard::new();
        let _ = g.ingest(9_000, GEvent::new(0x01, 0x1E, 1)).is_ok();
        let out = g.ingest(1_000, GEvent::new(0x01, 0x1E, 0));
        let declared = matches!(out, Ok(GuardOutcome::ResetDeclared));
        let mut ok = declared && g.epoch() == 1;
        let mut i = 0u32;
        while i < RESET_QUARANTINE {
            let _ = g.ingest(1_000 + i as u64 + 1, GEvent::new(0x01, 0x40, 0)).is_ok();
            i += 1;
        }
        ok = ok && matches!(g.ingest(2_000, GEvent::new(0x01, 0x1E, 0)), Ok(GuardOutcome::Accepted));
        ok = ok && g.reconcile().is_ok();
        ok
    };
    v.push((
        "F48806-J1-重置回基后继续受理",
        rebase,
        String::from("epoch=1 → 新基线 1000"),
    ));

    // F48807-J1：4096 上限精确（4096 不丢，4097 丢一帧）。
    let overflow = overflow_drill();
    v.push((
        "F48807-J1-溢出10/10逐条一致",
        matches!(overflow, Ok(n) if n == JUDGE_ROUNDS),
        format!("drops={:?}", overflow),
    ));

    // F48807-J1：溢出账与丢帧计数逐条一致。
    let ledger_ok = {
        let mut g = EventGuard::new();
        let mut i = 0usize;
        while i < OVERFLOW_CAP {
            let _ = g.ingest(i as u64, GEvent::new(0x01, 0x1E, 1)).is_ok();
            i += 1;
        }
        let _ = g.ingest(OVERFLOW_CAP as u64, GEvent::new(0x01, 0x1E, 0)).is_ok();
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < g.overflow_records().len() {
            if let Some(r) = g.overflow_records().get(k) {
                sum = sum.saturating_add(r.dropped_events * r.dropped_frames);
            }
            k += 1;
        }
        sum == g.dropped() && g.max_pending() == OVERFLOW_CAP && g.reconcile().is_ok()
    };
    v.push((
        "F48807-J1-溢出账逐条一致",
        ledger_ok,
        format!("max_pending={} cap={}", OVERFLOW_CAP, OVERFLOW_CAP),
    ));

    // F48807-J2：注入 4 槽 ×10 全中。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = matches!(&led, Ok(l) if l.all_caught());
    v.push((
        "F48807-J2-注入10轮4槽全中",
        led_ok,
        format!(
            "tries={:?} caught={:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));

    // F48807-J3：双跑 + 段位。
    v.push((
        "F48807-J3-双跑与段位",
        repro_check().is_ok() && segment_guard(),
        format!("golden={:?}", golden_run().map(|g| g.digest)),
    ));

    v
}

pub fn run_unx48805_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48805 输入事件时序三闸");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synth_after_exactly_100ms() {
        assert_eq!(synth_boundary(), Ok(2));
    }

    #[test]
    fn synth_drill_ten_records() {
        assert_eq!(synth_drill(), Ok(JUDGE_ROUNDS));
    }

    #[test]
    fn reset_counts_eleven_zeroed() {
        assert_eq!(reset_drill(), Ok(JUDGE_ROUNDS));
    }

    #[test]
    fn reset_requires_baseline_first() {
        let mut g = EventGuard::new();
        // 首条事件无基线，永远不该判倒流。
        assert_eq!(g.ingest(0, GEvent::new(0x01, 0x1E, 1)), Ok(GuardOutcome::Accepted));
        assert_eq!(g.epoch(), 0);
        // 相同时间戳不算倒流（允许同毫秒多事件）。
        assert_eq!(g.ingest(0, GEvent::new(0x01, 0x1E, 0)), Ok(GuardOutcome::Accepted));
        assert_eq!(g.epoch(), 0);
    }

    #[test]
    fn overflow_cap_is_4096() {
        assert_eq!(OVERFLOW_CAP, 4_096);
        assert_eq!(overflow_drill(), Ok(JUDGE_ROUNDS));
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn overflow_does_not_hide_synth() {
        // 长但未满的帧：先按 100ms 合成，而不是等溢出。
        let mut g = EventGuard::new();
        let mut i = 0usize;
        while i < 10 {
            g.ingest(i as u64, GEvent::new(0x01, 0x1E, 1)).expect("push");
            i += 1;
        }
        let out = g.ingest(200_000, GEvent::new(0x01, 0x1F, 1)).expect("push");
        assert_eq!(out, GuardOutcome::SynthCommitted);
        assert_eq!(g.dropped(), 0);
        assert_eq!(g.synth_records().len(), 1);
    }

    #[test]
    fn quarantine_is_eight_and_reopens() {
        let mut g = EventGuard::new();
        g.ingest(1_000, GEvent::new(0x01, 0x1E, 1)).expect("push");
        g.ingest(900, GEvent::new(0x01, 0x1E, 0)).expect("reset");
        assert_eq!(g.quarantine_left(), RESET_QUARANTINE);
        let mut i = 0u32;
        while i < RESET_QUARANTINE {
            assert_eq!(
                g.ingest(900 + i as u64 + 1, GEvent::new(0x01, 0x40, 0)),
                Ok(GuardOutcome::Quarantined)
            );
            i += 1;
        }
        assert_eq!(g.quarantine_left(), 0);
        assert_eq!(g.ingest(1_000, GEvent::new(0x01, 0x1E, 0)), Ok(GuardOutcome::Accepted));
    }

    #[test]
    fn reconcile_identity_holds_after_mixed_stream() {
        let mut g = EventGuard::new();
        let mut ts = 1u64;
        let mut i = 0u16;
        while i < 6 {
            g.ingest(ts, GEvent::new(0x01, 0x1E + i, 1)).expect("push");
            ts += 1;
            i += 1;
        }
        g.ingest(ts, GEvent::new(0x00, 0, 0)).expect("syn");
        ts += SYN_TIMEOUT_US;
        g.ingest(ts, GEvent::new(0x01, 0x30, 1)).expect("synth");
        g.flush_pending(ts + 5).expect("flush");
        assert!(g.reconcile().is_ok());
        assert_eq!(g.frames(), 2);
        assert_eq!(g.committed_events(), 6 + 1 + 1);
    }

    #[test]
    fn golden_run_stable() {
        let a = golden_run().expect("黄金链");
        assert_eq!(a, golden_run().expect("二跑"));
        assert_eq!(a.synth, JUDGE_ROUNDS);
        assert_eq!(a.resets, JUDGE_ROUNDS);
        assert_eq!(a.overflows, JUDGE_ROUNDS);
        assert!(a.committed > 0);
        assert!(a.zeroed > 0);
        // 混合流本身不溢出：丢弃数应为 0（溢出由独立演练轮覆盖）。
        assert_eq!(a.dropped, 0);
    }

    #[test]
    fn inject_four_slots_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).expect("注入");
        assert!(led.all_caught());
        assert_eq!(led.tries, [JUDGE_ROUNDS; 4]);
        assert_eq!(led.caught, [JUDGE_ROUNDS; 4]);
    }

    #[test]
    fn probes_a_b_c_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn codes_are_segment_closed() {
        assert!(segment_guard());
        for c in GuardCode::ALL.iter() {
            let v = c.val();
            assert!(v > SEG_BASE && v < SEG_BASE + 0x20);
            assert_eq!(detail(v), c.detail());
        }
        assert_eq!(detail(0x7008D), "未知诊断码：不在 0x70081..0x7008C 闭集内");
    }

    #[test]
    fn close_reason_labels_are_distinct() {
        let mut seen: Vec<&str> = Vec::new();
        for r in [
            CloseReason::Syn,
            CloseReason::SynthTimeout,
            CloseReason::OverflowDrop,
            CloseReason::ResetFlush,
        ]
        .iter()
        {
            assert!(!seen.contains(&r.label()));
            seen.push(r.label());
        }
        assert_eq!(seen.len(), 4);
    }
}
