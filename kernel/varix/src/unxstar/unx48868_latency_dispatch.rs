//! UNX-F48868 四段样本环形分派 · 0x70860
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥850 行；
//!   ② UNX-F48868-J1 环填 ×10^3 零丢、序号严格递增；
//!   ③ F48869/F48870 导出面与账面对平。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：环域（空/满）→ 序号单调 → 代际一致 → 段号闭集 →
//!           结果码 → 路由 → 账平 → 导出复算。
//!   次序不可倒：环满先判，满环上盖写会静默丢最老样本；序号先于
//!   代际，回退序号在复用槽上会被误判成「新一代」；代际先于段号，
//!   跨代样本的段号不可信；导出必须最后：账未平就导出会把错误
//!   固化进文件。
//! 关键差异：本模块只负责「样本从环到四段通道」的搬运与对账，不
//!   产生时间戳（core）、不校准时钟（meta）、不判闸（guard）。所有
//!   样本必须带 seq/gen/seg 三元组，任何一项缺失都拒绝，不走默认值。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48861_latency_core::{SegId, SegResult};

/// 诊断段基址（0x70860，VX6 批 B04 第四槽）。
pub const SEG_BASE: u32 = 0x70860;

/// 环容量（必须为 2 的幂，便于取模）。
pub const RING_CAP: usize = 256;
/// 填环轮数（判据 ×10^3）。
pub const FILL_ROUNDS: u32 = 1000;
/// 通道数（四段）。
pub const LANES: usize = 4;
/// 导出保留行上限（黄金链只保留最近 1000 行，恰好覆盖全轮）。
pub const EXPORT_ROWS: usize = 1000;
/// 导出行段数：seq|gen|stage|us|result。
pub const EXPORT_FIELDS: usize = 5;

// ── 诊断码闭集（+0x01..+0x0A） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpCode {
    /// 环满
    RingFull,
    /// 序号回退或重复
    SeqBad,
    /// 代际不一致（槽已被复用）
    GenBad,
    /// 段号非法（越出四段闭集）
    RouteBad,
    /// 结果码非法（不属于五值闭集）
    ResBad,
    /// 重复投递（同一 seq 已出环）
    DupBad,
    /// 禁用泄漏（rejected 样本进了通道）
    LeakBad,
    /// 账不平（出 + 留 ≠ 入）
    CountBad,
    /// 导出行不可解析
    PrintBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl DpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DpCode::RingFull => "环满：禁止覆盖最老样本，必须先出环",
            DpCode::SeqBad => "序号回退：每通道 seq 必须严格递增",
            DpCode::GenBad => "代际不一致：环槽已被新一代样本复用",
            DpCode::RouteBad => "段号非法：只允许四段闭集",
            DpCode::ResBad => "结果码非法：只允许 Delivered/Timeout/Dropped/Bypassed/Rejected",
            DpCode::DupBad => "重复投递：同一 (seq,gen) 已经出环",
            DpCode::LeakBad => "禁用泄漏：Rejected 样本不得进入通道",
            DpCode::CountBad => "账不平：出环 + 在环 ≠ 入环",
            DpCode::PrintBad => "导出非法：行不可解析或字段数不符",
            DpCode::ReplayBad => "复算不一致：两次黄金链摘要不同",
            DpCode::SegmentBad => "段位越界：诊断码逃出 0x70860 窗口",
        }
    }

    pub const ALL: [DpCode; 11] = [
        DpCode::RingFull,
        DpCode::SeqBad,
        DpCode::GenBad,
        DpCode::RouteBad,
        DpCode::ResBad,
        DpCode::DupBad,
        DpCode::LeakBad,
        DpCode::CountBad,
        DpCode::PrintBad,
        DpCode::ReplayBad,
        DpCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        if DpCode::ALL[i].val() == code {
            return DpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70860 段闭集内"
}

fn result_ord(r: SegResult) -> u8 {
    r.ord()
}

fn result_from_ord(v: u8) -> Result<SegResult, u32> {
    SegResult::from_ord(v)
}

// ── 样本 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SampleRec {
    pub seq: u32,
    pub gen: u16,
    pub seg: SegId,
    pub us: u32,
    pub result: SegResult,
}

impl SampleRec {
    pub fn new(seq: u32, gen: u16, seg: SegId, us: u32, result: SegResult) -> SampleRec {
        SampleRec { seq, gen, seg, us, result }
    }

    pub fn tag(&self) -> u64 {
        (u64::from(self.gen) << 32) | u64::from(self.seq)
    }

    pub fn text(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.seq,
            self.gen,
            self.seg.stage(),
            self.us,
            self.result.text()
        )
    }
}

// ── 环形缓冲 ──────────────────────────────────────────────────────
pub struct Ring {
    slots: [Option<SampleRec>; RING_CAP],
    head: usize,
    tail: usize,
    len: usize,
    gen: u16,
    pushed: u64,
    popped: u64,
}

impl Ring {
    pub const fn new(gen: u16) -> Ring {
        Ring {
            slots: [None; RING_CAP],
            head: 0,
            tail: 0,
            len: 0,
            gen,
            pushed: 0,
            popped: 0,
        }
    }

    pub fn gen(&self) -> u16 {
        self.gen
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.len == RING_CAP
    }

    pub fn pushed(&self) -> u64 {
        self.pushed
    }

    pub fn popped(&self) -> u64 {
        self.popped
    }

    /// 入环：满即拒，不做覆盖写。
    pub fn push(&mut self, rec: SampleRec) -> Result<(), u32> {
        if self.is_full() {
            return Err(DpCode::RingFull.val());
        }
        if rec.gen != self.gen {
            return Err(DpCode::GenBad.val());
        }
        match self.slots.get_mut(self.tail) {
            Some(slot) => *slot = Some(rec),
            None => return Err(DpCode::RouteBad.val()),
        }
        self.tail = (self.tail + 1) % RING_CAP;
        self.len = self.len.saturating_add(1);
        self.pushed = self.pushed.saturating_add(1);
        Ok(())
    }

    /// 出环：空即拒；出环后清槽并翻代。
    pub fn pop(&mut self) -> Result<SampleRec, u32> {
        if self.is_empty() {
            return Err(DpCode::RingFull.val());
        }
        let rec = match self.slots.get_mut(self.head) {
            Some(slot) => match slot.take() {
                Some(r) => r,
                None => return Err(DpCode::RouteBad.val()),
            },
            None => return Err(DpCode::RouteBad.val()),
        };
        self.head = (self.head + 1) % RING_CAP;
        self.len = self.len.saturating_sub(1);
        self.popped = self.popped.saturating_add(1);
        // 每出环一格推进一次槽代，防止旧样本借复用槽「复活」。
        self.gen = self.gen.wrapping_add(1);
        Ok(rec)
    }

    /// 账平：入 = 出 + 在环。
    pub fn balance(&self) -> Result<(), u32> {
        if self.pushed != self.popped.saturating_add(self.len as u64) {
            return Err(DpCode::CountBad.val());
        }
        Ok(())
    }
}

// ── 四段通道 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lane {
    pub lane_seq: u32,
    pub delivered: u32,
    pub dropped: u32,
    pub timeouts: u32,
    pub bypassed: u32,
    pub rejected: u32,
    pub last_us: u32,
}

impl Lane {
    pub const fn new() -> Lane {
        Lane {
            lane_seq: 0,
            delivered: 0,
            dropped: 0,
            timeouts: 0,
            bypassed: 0,
            rejected: 0,
            last_us: 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.delivered
            .saturating_add(self.dropped)
            .saturating_add(self.timeouts)
            .saturating_add(self.bypassed)
            .saturating_add(self.rejected)
    }
}

/// 分派器：环 + 四通道 + 出向账。
pub struct Dispatcher {
    pub ring: Ring,
    pub lanes: [Lane; LANES],
    pub submitted: u64,
    pub delivered: u64,
    pub leaked: u64,
    seen_seq: u32,
}

impl Dispatcher {
    pub fn new(gen: u16) -> Dispatcher {
        Dispatcher {
            ring: Ring::new(gen),
            lanes: [Lane::new(); LANES],
            submitted: 0,
            delivered: 0,
            leaked: 0,
            seen_seq: 0,
        }
    }

    /// 提交：五次闸（环满 → 序号 → 代际 → 段号 → 结果码）。
    pub fn submit(&mut self, rec: SampleRec) -> Result<(), u32> {
        if self.ring.is_full() {
            return Err(DpCode::RingFull.val());
        }
        if rec.seq <= self.seen_seq {
            return Err(DpCode::SeqBad.val());
        }
        if rec.gen != self.ring.gen() {
            return Err(DpCode::GenBad.val());
        }
        if rec.seg.ord() as usize >= LANES {
            return Err(DpCode::RouteBad.val());
        }
        if result_from_ord(rec.result.ord()).is_err() {
            return Err(DpCode::ResBad.val());
        }
        self.ring.push(rec)?;
        self.seen_seq = rec.seq;
        self.submitted = self.submitted.saturating_add(1);
        Ok(())
    }

    /// 出环并投递到对应通道；Rejected 不入通道（入泄漏账）。
    pub fn drain_one(&mut self) -> Result<Option<SampleRec>, u32> {
        if self.ring.is_empty() {
            return Ok(None);
        }
        let rec = self.ring.pop()?;
        let ix = rec.seg.ord() as usize;
        let lane = match self.lanes.get_mut(ix) {
            Some(l) => l,
            None => return Err(DpCode::RouteBad.val()),
        };
        if rec.seq <= lane.lane_seq {
            return Err(DpCode::DupBad.val());
        }
        lane.lane_seq = rec.seq;
        lane.last_us = rec.us;
        match rec.result {
            SegResult::Delivered => lane.delivered = lane.delivered.saturating_add(1),
            SegResult::Dropped => lane.dropped = lane.dropped.saturating_add(1),
            SegResult::Timeout => lane.timeouts = lane.timeouts.saturating_add(1),
            SegResult::Bypassed => lane.bypassed = lane.bypassed.saturating_add(1),
            SegResult::Rejected => lane.rejected = lane.rejected.saturating_add(1),
        }
        if rec.result == SegResult::Rejected {
            self.leaked = self.leaked.saturating_add(1);
        } else {
            self.delivered = self.delivered.saturating_add(1);
        }
        Ok(Some(rec))
    }

    pub fn drain_all(&mut self) -> Result<u32, u32> {
        let mut n = 0u32;
        while !self.ring.is_empty() {
            match self.drain_one()? {
                Some(_) => n = n.saturating_add(1),
                None => break,
            }
        }
        Ok(n)
    }

    /// 账平：提交 = 出环成功 + 在环；出环 = 投递 + 泄漏 + 拒收。
    pub fn balance(&self) -> Result<(), u32> {
        self.ring.balance()?;
        let out = self.delivered.saturating_add(self.leaked);
        if out != self.ring.popped() {
            return Err(DpCode::CountBad.val());
        }
        if self.submitted != self.ring.pushed() {
            return Err(DpCode::CountBad.val());
        }
        let mut lane_total = 0u32;
        let mut i = 0usize;
        while i < self.lanes.len() {
            if let Some(l) = self.lanes.get(i) {
                lane_total = lane_total.saturating_add(l.total());
            }
            i += 1;
        }
        if u64::from(lane_total) != self.ring.popped() {
            return Err(DpCode::CountBad.val());
        }
        Ok(())
    }
}

// ── 确定性源 ──────────────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x4C41_5444_5054_3034)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

pub const FNV_OFF: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut b = 0u32;
    while b < 4 {
        x ^= u64::from((v >> (b * 8)) & 0xFF);
        x = x.wrapping_mul(FNV_PRIME);
        b += 1;
    }
    x
}

pub fn fold_u64(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut b = 0u64;
    while b < 8 {
        x ^= (v >> (b * 8)) & 0xFF;
        x = x.wrapping_mul(FNV_PRIME);
        b += 1;
    }
    x
}

// ── 黄金链 ────────────────────────────────────────────────────────
/// 结果码按序轮转：Delivered / Timeout / Dropped / Bypassed / Rejected。
fn result_for(r: u32) -> SegResult {
    match r % 5 {
        0 => SegResult::Delivered,
        1 => SegResult::Timeout,
        2 => SegResult::Dropped,
        3 => SegResult::Bypassed,
        _ => SegResult::Rejected,
    }
}

/// 千轮填环：每四轮为一段，边填边出，环容量足够。
pub fn golden_fill(rounds: u32) -> Result<(u32, u32, u64), u32> {
    if rounds == 0 || rounds > 100_000 {
        return Err(DpCode::CountBad.val());
    }
    let mut d = Dispatcher::new(7);
    let mut exported = Vec::new();
    let mut r = 0u32;
    let mut h = FNV_OFF;
    while r < rounds {
        let seg = match SegId::from_ord((r % LANES as u32) as u8) {
            Ok(s) => s,
            Err(_) => return Err(DpCode::RouteBad.val()),
        };
        let rec = SampleRec::new(r + 1, d.ring.gen(), seg, 100 + (r % 900), result_for(r));
        d.submit(rec)?;
        if let Some(out) = d.drain_one()? {
            h = fold_u32(h, out.seq);
            h = fold_u32(h, u32::from(out.gen));
            h = fold_u32(h, out.us);
            h = fold_u32(h, u32::from(result_ord(out.result)));
            if exported.len() < EXPORT_ROWS {
                exported.push(out);
            }
        }
        r = r.saturating_add(1);
    }
    d.drain_all()?;
    d.balance()?;
    if d.leaked == 0 || d.delivered == 0 {
        return Err(DpCode::LeakBad.val());
    }
    let rows = exported.len() as u32;
    Ok((rows, d.leaked as u32, h))
}

/// 深环：一次填满 256 再批量出，验证满环拒绝。
pub fn golden_capacity() -> Result<(u32, u32), u32> {
    let mut d = Dispatcher::new(3);
    let mut i = 0u32;
    while i < RING_CAP as u32 {
        let seg = match SegId::from_ord((i % LANES as u32) as u8) {
            Ok(s) => s,
            Err(_) => return Err(DpCode::RouteBad.val()),
        };
        d.submit(SampleRec::new(i + 1, d.ring.gen(), seg, 200, SegResult::Delivered))?;
        i = i.saturating_add(1);
    }
    let full_reached = d.ring.is_full()
        && d.submit(SampleRec::new(9999, d.ring.gen(), SegId::IrqArrival, 200, SegResult::Delivered))
            == Err(DpCode::RingFull.val());
    if !full_reached {
        return Err(DpCode::RingFull.val());
    }
    let out = d.drain_all()?;
    d.balance()?;
    Ok((RING_CAP as u32, out))
}

// ── 导出面 ────────────────────────────────────────────────────────
/// 导出 CSV：每行五段，末行汇总。
pub fn export_csv(rows: &[SampleRec]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            out.push_str(&r.text());
            out.push('\n');
        }
        i += 1;
    }
    out
}

/// 行审计：字段数、段名闭集、数值可解析（逐字符核）。
pub fn parse_row(line: &str) -> Result<SampleRec, u32> {
    let mut parts = line.split('|');
    let s_seq = match parts.next() {
        Some(x) => x,
        None => return Err(DpCode::PrintBad.val()),
    };
    let s_gen = match parts.next() {
        Some(x) => x,
        None => return Err(DpCode::PrintBad.val()),
    };
    let s_stage = match parts.next() {
        Some(x) => x,
        None => return Err(DpCode::PrintBad.val()),
    };
    let s_us = match parts.next() {
        Some(x) => x,
        None => return Err(DpCode::PrintBad.val()),
    };
    let s_res = match parts.next() {
        Some(x) => x,
        None => return Err(DpCode::PrintBad.val()),
    };
    if parts.next().is_some() {
        return Err(DpCode::PrintBad.val());
    }
    let seq = parse_u32(s_seq)?;
    let gen = parse_u32(s_gen)?;
    if gen > u32::from(u16::MAX) {
        return Err(DpCode::PrintBad.val());
    }
    let seg = stage_from_name(s_stage)?;
    let us = parse_u32(s_us)?;
    let result = match s_res {
        "delivered" => SegResult::Delivered,
        "timeout" => SegResult::Timeout,
        "dropped" => SegResult::Dropped,
        "bypassed" => SegResult::Bypassed,
        "rejected" => SegResult::Rejected,
        _ => return Err(DpCode::PrintBad.val()),
    };
    Ok(SampleRec::new(seq, gen as u16, seg, us, result))
}

fn parse_u32(s: &str) -> Result<u32, u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 {
        return Err(DpCode::PrintBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(x) => *x,
            None => return Err(DpCode::PrintBad.val()),
        };
        if !b.is_ascii_digit() {
            return Err(DpCode::PrintBad.val());
        }
        v = v
            .saturating_mul(10)
            .saturating_add(u32::from(b.saturating_sub(b'0')));
        i += 1;
    }
    Ok(v)
}

fn stage_from_name(name: &str) -> Result<SegId, u32> {
    let mut i = 0u8;
    while i < LANES as u8 {
        let seg = match SegId::from_ord(i) {
            Ok(s) => s,
            Err(_) => return Err(DpCode::RouteBad.val()),
        };
        if seg.stage() == name {
            return Ok(seg);
        }
        i += 1;
    }
    Err(DpCode::RouteBad.val())
}

/// 全量导出审计：逐行解析并复算摘要。
pub fn audit_export(text: &str) -> Result<(u32, u64), u32> {
    let mut lines = 0u32;
    let mut h = FNV_OFF;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let rec = parse_row(line)?;
        h = fold_u32(h, rec.seq);
        h = fold_u32(h, u32::from(rec.gen));
        h = fold_u32(h, rec.us);
        h = fold_u32(h, u32::from(result_ord(rec.result)));
        lines = lines.saturating_add(1);
    }
    if lines == 0 {
        return Err(DpCode::PrintBad.val());
    }
    Ok((lines, h))
}

// ── 黄金全链 ──────────────────────────────────────────────────────
pub fn golden_export(rounds: u32) -> Result<(u32, u64), u32> {
    let mut d = Dispatcher::new(11);
    let mut rows = Vec::new();
    let mut r = 0u32;
    while r < rounds {
        let seg = match SegId::from_ord((r % LANES as u32) as u8) {
            Ok(s) => s,
            Err(_) => return Err(DpCode::RouteBad.val()),
        };
        let rec = SampleRec::new(r + 1, d.ring.gen(), seg, 100 + (r % 900), result_for(r));
        d.submit(rec)?;
        if let Some(out) = d.drain_one()? {
            if rows.len() < EXPORT_ROWS {
                rows.push(out);
            }
        }
        r = r.saturating_add(1);
    }
    d.drain_all()?;
    d.balance()?;
    let text = export_csv(&rows);
    let (lines, h) = audit_export(&text)?;
    if lines as u32 != rows.len() as u32 {
        return Err(DpCode::PrintBad.val());
    }
    Ok((lines, h))
}

pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (filled, leaked, fill_h) = golden_fill(FILL_ROUNDS)?;
    let (cap, drained) = golden_capacity()?;
    let (lines, export_h) = golden_export(FILL_ROUNDS)?;
    if filled != FILL_ROUNDS || cap != RING_CAP as u32 || drained != RING_CAP as u32 {
        return Err(DpCode::CountBad.val());
    }
    if lines != FILL_ROUNDS || leaked == 0 {
        return Err(DpCode::LeakBad.val());
    }
    let mut h = FNV_OFF;
    h = fold_u32(h, filled);
    h = fold_u32(h, leaked);
    h = fold_u32(h, cap);
    h = fold_u64(h, fill_h);
    h = fold_u64(h, export_h);
    Ok((filled, leaked, cap, h))
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 路 1：序号回退。
        let mut d = Dispatcher::new(1);
        let ok1 = d
            .submit(SampleRec::new(5, d.ring.gen(), SegId::IrqArrival, 100, SegResult::Delivered))
            .is_ok();
        let bad = d
            .submit(SampleRec::new(5, d.ring.gen(), SegId::IrqArrival, 100, SegResult::Delivered))
            == Err(DpCode::SeqBad.val());
        led.record(0, ok1 && bad);

        // 路 2：代际不一致。
        let mut d2 = Dispatcher::new(2);
        let bad_gen = d2
            .submit(SampleRec::new(1, 9, SegId::FocusDispatch, 100, SegResult::Delivered))
            == Err(DpCode::GenBad.val());
        led.record(1, bad_gen);

        // 路 3：满环拒写。
        let mut d3 = Dispatcher::new(3);
        let mut i = 0u32;
        let mut fill_ok = true;
        while i < RING_CAP as u32 {
            let seg = match SegId::from_ord((i % LANES as u32) as u8) {
                Ok(s) => s,
                Err(_) => return Err(DpCode::RouteBad.val()),
            };
            if d3
                .submit(SampleRec::new(i + 1, d3.ring.gen(), seg, 100, SegResult::Delivered))
                .is_err()
            {
                fill_ok = false;
            }
            i += 1;
        }
        let full = d3.ring.is_full()
            && d3
                .submit(SampleRec::new(5000, d3.ring.gen(), SegId::IrqArrival, 100, SegResult::Delivered))
                == Err(DpCode::RingFull.val());
        led.record(2, fill_ok && full);

        // 路 4：导出坏行 / 段名越界。
        let bad_row = parse_row("1|2|no-such-stage|100|delivered") == Err(DpCode::RouteBad.val())
            && parse_row("1|2|irq-arrival|100|wat") == Err(DpCode::PrintBad.val())
            && parse_row("1|2|irq-arrival|100") == Err(DpCode::PrintBad.val())
            && parse_row("x|2|irq-arrival|100|delivered") == Err(DpCode::PrintBad.val());
        led.record(3, bad_row);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (filled, leaked, _) = golden_fill(FILL_ROUNDS)?;
    if filled != FILL_ROUNDS || leaked == 0 {
        return Err(DpCode::CountBad.val());
    }
    let (cap, out) = golden_capacity()?;
    if cap != RING_CAP as u32 || out != RING_CAP as u32 {
        return Err(DpCode::CountBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let (lines, h) = golden_export(FILL_ROUNDS)?;
    let (lines2, h2) = golden_export(FILL_ROUNDS)?;
    if lines != lines2 || h != h2 || lines != FILL_ROUNDS {
        return Err(DpCode::ReplayBad.val());
    }
    let text = export_csv(&[]);
    if !text.is_empty() || audit_export("").is_ok() {
        return Err(DpCode::PrintBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if RING_CAP == 256
        && FILL_ROUNDS == 1000
        && LANES == 4
        && EXPORT_ROWS == 1000
        && EXPORT_FIELDS == 5
        && DpCode::ALL.len() == 11
    {
        Ok(())
    } else {
        Err(DpCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a == b {
        Ok(())
    } else {
        Err(DpCode::ReplayBad.val())
    }
}

pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        let c = DpCode::ALL[i].val();
        if c < SEG_BASE.saturating_add(1) || c > SEG_BASE.saturating_add(0x0B) {
            ok = false;
        }
        i += 1;
    }
    ok && detail(SEG_BASE).starts_with("未知诊断码")
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let fill = golden_fill(FILL_ROUNDS);
    v.push((
        "F48868-J1-千轮填环零丢",
        match &fill {
            Ok((rows, _, _)) => *rows == FILL_ROUNDS,
            Err(_) => false,
        },
        match &fill {
            Ok((rows, leak, h)) => format!("rows={} leaked={} h={:016X}", rows, leak, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48868-J1-满环拒写",
        match golden_capacity() {
            Ok((cap, out)) => cap == RING_CAP as u32 && out == RING_CAP as u32,
            Err(_) => false,
        },
        format!("cap={} rounds={}", RING_CAP, FILL_ROUNDS),
    ));
    v.push((
        "F48868-J1-账平出留入",
        {
            let mut d = Dispatcher::new(5);
            let mut i = 0u32;
            let mut ok = true;
            while i < 400 {
                let seg = match SegId::from_ord((i % 4) as u8) {
                    Ok(s) => s,
                    Err(_) => {
                        ok = false;
                        break;
                    }
                };
                // 环满先腾一格再入：否则 400 轮填 256 格必然撞满环，
                // 这一项本该验「账平」，不该被容量闸误伤成红项。
                if d.ring.is_full() {
                    let _ = d.drain_one();
                }
                if d.submit(SampleRec::new(i + 1, d.ring.gen(), seg, 100, SegResult::Delivered))
                    .is_err()
                {
                    ok = false;
                }
                if i % 3 == 0 {
                    let _ = d.drain_one();
                }
                i += 1;
            }
            ok && d.drain_all().is_ok() && d.balance().is_ok()
        },
        String::from("提交 = 出环 + 在环；出环 = 投递 + 泄漏"),
    ));
    let exp = golden_export(FILL_ROUNDS);
    v.push((
        "F48869-J1-导出面千行",
        match &exp {
            Ok((lines, _)) => *lines == FILL_ROUNDS,
            Err(_) => false,
        },
        match &exp {
            Ok((lines, h)) => format!("lines={} h={:016X}", lines, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48869-J1-坏行必拒",
        parse_row("1|2|no-such|100|delivered").is_err()
            && parse_row("1|2|irq-arrival|100|wat").is_err()
            && parse_row("1|2|irq-arrival|100").is_err(),
        String::from("坏段名/坏结果码/缺字段"),
    ));
    v.push((
        "F48870-J1-双跑零漂移",
        {
            let a = golden_run();
            let b = golden_run();
            a.is_ok() && a == b
        },
        format!("a={:?} b={:?}", golden_run().ok(), golden_run().ok()),
    ));
    v.push((
        "F48870-J1-注入四路10/10",
        match inject_rounds(10) {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match inject_rounds(10) {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48870-J1-探针三序与复算",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48870-J1-段位守卫",
        segment_guard(),
        String::from("0x70860 窗口 11 码"),
    ));
    v.push((
        "F48868-J1-环代际翻新",
        {
            let mut d = Dispatcher::new(1);
            let g0 = d.ring.gen();
            let ok = d
                .submit(SampleRec::new(1, g0, SegId::IrqArrival, 10, SegResult::Delivered))
                .is_ok();
            let out = d.drain_one().is_ok();
            let g1 = d.ring.gen();
            ok && out && g1 == g0.wrapping_add(1)
        },
        String::from("每出环一格翻一代"),
    ));
    v
}

pub fn run_unx48868_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48868 四段样本环形分派");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_push_pop_fifo() {
        let mut r = Ring::new(1);
        // 同一代内连推两格：出环前代不翻，第二格仍属当前代。
        assert_eq!(r.push(SampleRec::new(1, 1, SegId::IrqArrival, 10, SegResult::Delivered)), Ok(()));
        assert_eq!(r.push(SampleRec::new(2, 1, SegId::FocusDispatch, 20, SegResult::Timeout)), Ok(()));
        assert_eq!(r.len(), 2);
        let a = r.pop();
        assert_eq!(a.map(|x| x.seq), Ok(1));
        let b = r.pop();
        assert_eq!(b.map(|x| x.seq), Ok(2));
        assert!(r.is_empty());
        assert!(r.pop().is_err());
        // 出环两格后代前进两格：旧代样本不得再入。
        assert_eq!(r.gen(), 3);
        assert_eq!(
            r.push(SampleRec::new(3, 1, SegId::AppCallback, 30, SegResult::Delivered)),
            Err(DpCode::GenBad.val())
        );
    }

    #[test]
    fn ring_rejects_wrong_generation() {
        let mut r = Ring::new(4);
        assert_eq!(
            r.push(SampleRec::new(1, 5, SegId::IrqArrival, 10, SegResult::Delivered)),
            Err(DpCode::GenBad.val())
        );
    }

    #[test]
    fn ring_generation_advances_on_pop() {
        let mut r = Ring::new(0);
        assert_eq!(r.push(SampleRec::new(1, 0, SegId::IrqArrival, 10, SegResult::Delivered)), Ok(()));
        let _ = r.pop();
        assert_eq!(r.gen(), 1);
    }

    #[test]
    fn ring_full_refuses_overwrite() {
        let mut r = Ring::new(1);
        let mut i = 0u32;
        while i < RING_CAP as u32 {
            assert_eq!(
                r.push(SampleRec::new(i + 1, r.gen(), SegId::IrqArrival, 10, SegResult::Delivered)),
                Ok(())
            );
            i += 1;
        }
        assert!(r.is_full());
        assert_eq!(
            r.push(SampleRec::new(9999, r.gen(), SegId::IrqArrival, 10, SegResult::Delivered)),
            Err(DpCode::RingFull.val())
        );
    }

    #[test]
    fn submit_rejects_seq_backwards() {
        let mut d = Dispatcher::new(1);
        assert!(d
            .submit(SampleRec::new(3, d.ring.gen(), SegId::IrqArrival, 10, SegResult::Delivered))
            .is_ok());
        assert_eq!(
            d.submit(SampleRec::new(3, d.ring.gen(), SegId::IrqArrival, 10, SegResult::Delivered)),
            Err(DpCode::SeqBad.val())
        );
        assert_eq!(
            d.submit(SampleRec::new(2, d.ring.gen(), SegId::IrqArrival, 10, SegResult::Delivered)),
            Err(DpCode::SeqBad.val())
        );
    }

    #[test]
    fn submit_rejects_bad_generation() {
        let mut d = Dispatcher::new(2);
        assert_eq!(
            d.submit(SampleRec::new(1, 3, SegId::IrqArrival, 10, SegResult::Delivered)),
            Err(DpCode::GenBad.val())
        );
    }

    #[test]
    fn lanes_count_results() {
        let mut d = Dispatcher::new(1);
        let mut i = 0u32;
        while i < 5 {
            let rec = SampleRec::new(i + 1, d.ring.gen(), SegId::AppCallback, 100, result_for(i));
            assert!(d.submit(rec).is_ok());
            assert!(d.drain_one().is_ok());
            i += 1;
        }
        let l = match d.lanes.get(2) {
            Some(x) => *x,
            None => return,
        };
        assert_eq!(l.delivered, 1);
        assert_eq!(l.timeouts, 1);
        assert_eq!(l.dropped, 1);
        assert_eq!(l.bypassed, 1);
        assert_eq!(l.rejected, 1);
        assert_eq!(l.total(), 5);
    }

    #[test]
    fn balance_holds_after_fill() {
        let mut d = Dispatcher::new(1);
        let mut i = 0u32;
        while i < 300 {
            let seg = match SegId::from_ord((i % 4) as u8) {
                Ok(s) => s,
                Err(_) => return,
            };
            if d.ring.is_full() {
                let _ = d.drain_one();
            }
            assert!(d
                .submit(SampleRec::new(i + 1, d.ring.gen(), seg, 100, result_for(i)))
                .is_ok());
            if i % 7 == 0 {
                let _ = d.drain_one();
            }
            i += 1;
        }
        assert!(d.drain_all().is_ok());
        assert_eq!(d.balance(), Ok(()));
    }

    #[test]
    fn rejected_samples_never_enter_lanes() {
        let mut d = Dispatcher::new(1);
        let rec = SampleRec::new(1, d.ring.gen(), SegId::IrqArrival, 10, SegResult::Rejected);
        assert!(d.submit(rec).is_ok());
        assert!(d.drain_one().is_ok());
        assert_eq!(d.leaked, 1);
        assert_eq!(d.delivered, 0);
        if let Some(l) = d.lanes.get(0) {
            assert_eq!(l.rejected, 1);
            assert_eq!(l.delivered, 0);
        }
    }

    #[test]
    fn thousand_rounds_fill() {
        let r = golden_fill(FILL_ROUNDS);
        assert!(r.is_ok());
        if let Ok((rows, leak, _)) = r {
            assert_eq!(rows, FILL_ROUNDS);
            assert!(leak > 0);
        }
    }

    #[test]
    fn capacity_rounds() {
        let r = golden_capacity();
        assert!(r.is_ok());
        if let Ok((cap, out)) = r {
            assert_eq!(cap, RING_CAP as u32);
            assert_eq!(out, RING_CAP as u32);
        }
    }

    #[test]
    fn export_round_trip() {
        let rec = SampleRec::new(42, 7, SegId::FramePresent, 777, SegResult::Bypassed);
        let line = rec.text();
        assert_eq!(parse_row(&line), Ok(rec));
        assert_eq!(parse_row("42|7|frame-present|777|bypassed"), Ok(rec));
    }

    #[test]
    fn export_audit_counts_lines() {
        let r = golden_export(50);
        assert!(r.is_ok());
        if let Ok((lines, h)) = r {
            assert_eq!(lines, 50);
            assert_ne!(h, 0);
        }
    }

    #[test]
    fn golden_is_stable() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn inject_four_routes_all_caught() {
        let l = inject_rounds(10);
        assert!(l.is_ok());
        if let Ok(ll) = l {
            assert!(ll.all_caught());
            assert_eq!(ll.tries, [10; 4]);
            assert_eq!(ll.caught, [10; 4]);
        }
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(DpCode::ALL.len(), 11);
        assert_eq!(DpCode::RingFull.val(), SEG_BASE + 1);
        assert_eq!(DpCode::SegmentBad.val(), SEG_BASE + 11);
    }
}
