#![allow(dead_code)]
//! cgb759_evcatch —— CGPU-F0759 网页体验事件捕获。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0759 网页体验事件捕获  <SEG 0x6A20>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0759；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌网页体验事件捕获管道，整数面）：
//!   1. 事件类型闭集五项（写死语义）：NAV 导航 / PAINT 绘制 /
//!      INPUT 输入 / CUSTOM 自定 / LIFE 生命周期；
//!   2. 记录四元组（seq/kind/ts/hash）：seq 单调且 >0；hash 为
//!      FNV-1a 32 位确定性校验和，冲刷后逐条复核；
//!   3. 去重与乱序：seq ≤ 已见高水位时，窗内（差 ≤8）查库去重、
//!      窗外判过旧拒收；全新高水位直接入账；
//!   4. 定容账本（写死 48 槽）：满则拒收新事件（溢出弃计数），
//!      守恒恒等式：总入账 = 已收 + 重放拒 + 过旧拒，
//!      且 已收 = 在库 + 溢出弃，类型账合计 = 已收；
//!   5. 状态机：Idle → Capturing → Flushed 单向；Flushed 后禁
//!      捕获/再冲刷/重启；时间戳单调不回退（倒挂红码）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6A20 段为本模块独占（SEG_BASE=0x6A20），判别式
//!   SEG_BASE+1..+N；与 0x6A00/0x6A40 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const EC_VERSION: &str = "cgb759_evcatch-v1";

/// 诊断段基址（0x6A20 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6A20;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6A21 事件类型号越界（0..=4 之外）。
pub const XC_BAD_KIND: u16 = SEG_BASE + 1;
/// 0x6A22 序号非法（0）。
pub const XC_BAD_SEQ: u16 = SEG_BASE + 2;
/// 0x6A23 重复序号（重放）。
pub const XC_DUP_SEQ: u16 = SEG_BASE + 3;
/// 0x6A24 序号过旧（滑窗之外）。
pub const XC_STALE_SEQ: u16 = SEG_BASE + 4;
/// 0x6A25 时间戳倒挂。
pub const XC_TS_BACKWARD: u16 = SEG_BASE + 5;
/// 0x6A26 管道满（捕获账容量上限）。
pub const XC_PIPE_FULL: u16 = SEG_BASE + 6;
/// 0x6A27 状态非法跃迁。
pub const XC_STATE_BAD: u16 = SEG_BASE + 7;
/// 0x6A28 空管道冲刷。
pub const XC_FLUSH_EMPTY: u16 = SEG_BASE + 8;
/// 0x6A29 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 9;
/// 0x6A2A 校验和不符。
pub const XC_HASH_MISMATCH: u16 = SEG_BASE + 10;
/// 0x6A2B 时间戳越上限。
pub const XC_BAD_TS: u16 = SEG_BASE + 11;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 事件类型闭集：0=NAV 1=PAINT 2=INPUT 3=CUSTOM 4=LIFE。
pub const K_NAV: usize = 0;
pub const K_PAINT: usize = 1;
pub const K_INPUT: usize = 2;
pub const K_CUSTOM: usize = 3;
pub const K_LIFE: usize = 4;
/// 类型数（写死：5）。
pub const KIND_CAP: usize = 5;
/// 管道容量（写死：48）。
pub const PIPE_CAP: usize = 48;
/// 乱序容忍窗（写死：8）。
pub const WINDOW: u32 = 8;
/// 时间戳上限（写死：40 亿 ms）。
pub const TS_MAX: u32 = 4_000_000_000;

/// 捕获事件记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvRecord {
    /// 单调序号（>0）。
    pub seq: u32,
    /// 类型号（0..=4）。
    pub kind: usize,
    /// 时间戳（ms）。
    pub ts: u32,
    /// 记录校验和（FNV-1a 32 位）。
    pub hash: u32,
}

/// FNV-1a 32 位校验和（确定性，逐字节）。
pub fn ev_hash(seq: u32, kind: usize, ts: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in seq.to_le_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for b in (kind as u32).to_le_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    for b in ts.to_le_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 记录字段合法性（类型/序号/时间戳三查）。
pub fn ev_legal(seq: u32, kind: usize, ts: u32) -> Result<(), u16> {
    if kind >= KIND_CAP {
        return Err(XC_BAD_KIND);
    }
    if seq == 0 {
        return Err(XC_BAD_SEQ);
    }
    if ts > TS_MAX {
        return Err(XC_BAD_TS);
    }
    Ok(())
}

/// 捕获管道状态机：Idle → Capturing → Flushed（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipeState {
    /// 未开捕。
    Idle,
    /// 捕获中。
    Capturing,
    /// 已冲刷。
    Flushed,
}

/// 事件捕获管道：定容 48 槽 + 去重序号窗 + 类型计数账。
/// 守恒恒等式：
///   总入账 = 已收 + 重放拒 + 过旧拒；
///   已收 = 在库 + 溢出弃；
///   类型账合计 = 已收（冲刷后仍成立，账不随出库消失）。
pub struct CapturePipe {
    buf: Vec<EvRecord>,
    state: PipeState,
    last_seq: u32,
    last_ts: u32,
    total_in: u32,
    dup_reject: u32,
    stale_reject: u32,
    overflow_drop: u32,
    kind_count: [u32; KIND_CAP],
}

impl CapturePipe {
    /// 空管道（Idle）。
    pub fn new() -> Self {
        CapturePipe {
            buf: Vec::new(),
            state: PipeState::Idle,
            last_seq: 0,
            last_ts: 0,
            total_in: 0,
            dup_reject: 0,
            stale_reject: 0,
            overflow_drop: 0,
            kind_count: [0; KIND_CAP],
        }
    }

    /// 开捕：Idle → Capturing；重复开捕红码。
    pub fn start(&mut self) -> Result<(), u16> {
        if self.state != PipeState::Idle {
            return Err(XC_STATE_BAD);
        }
        self.state = PipeState::Capturing;
        Ok(())
    }

    /// 已收数（= 总入账 − 重放拒 − 过旧拒）。
    pub fn accepted(&self) -> u32 {
        self.total_in - self.dup_reject - self.stale_reject
    }

    /// 捕获入账：校验 + 倒挂查 + 去重窗 + 定容；成功返回校验和。
    pub fn capture(&mut self, seq: u32, kind: usize, ts: u32) -> Result<u32, u16> {
        if self.state != PipeState::Capturing {
            return Err(XC_STATE_BAD);
        }
        ev_legal(seq, kind, ts)?;
        if self.total_in > 0 && ts < self.last_ts {
            return Err(XC_TS_BACKWARD);
        }
        self.total_in += 1;
        if seq <= self.last_seq {
            if seq + WINDOW > self.last_seq {
                // 窗内回退：允许乱序补录，但序号必须全新。
                if self.buf.iter().any(|e| e.seq == seq) {
                    self.dup_reject += 1;
                    return Err(XC_DUP_SEQ);
                }
            } else {
                self.stale_reject += 1;
                return Err(XC_STALE_SEQ);
            }
        }
        if self.buf.len() >= PIPE_CAP {
            self.overflow_drop += 1;
            return Err(XC_PIPE_FULL);
        }
        let h = ev_hash(seq, kind, ts);
        self.buf.push(EvRecord {
            seq,
            kind,
            ts,
            hash: h,
        });
        if seq > self.last_seq {
            self.last_seq = seq;
        }
        self.last_ts = ts;
        self.kind_count[kind] += 1;
        Ok(h)
    }

    /// 在库数。
    pub fn stored(&self) -> usize {
        self.buf.len()
    }

    /// 已见高水位序号。
    pub fn high_water(&self) -> u32 {
        self.last_seq
    }

    /// 单条记录复核：hash 与字段重算一致。
    pub fn verify(rec: &EvRecord) -> Result<(), u16> {
        if ev_hash(rec.seq, rec.kind, rec.ts) != rec.hash {
            return Err(XC_HASH_MISMATCH);
        }
        Ok(())
    }

    /// 冲刷：按序号升序导出全部在库记录，Capturing → Flushed。
    pub fn flush(&mut self) -> Result<Vec<EvRecord>, u16> {
        if self.state != PipeState::Capturing {
            return Err(XC_STATE_BAD);
        }
        if self.buf.is_empty() {
            return Err(XC_FLUSH_EMPTY);
        }
        let mut v = core::mem::take(&mut self.buf);
        // 插入排序按 seq 升序（确定性）。
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1].seq > key.seq {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        self.state = PipeState::Flushed;
        Ok(v)
    }

    /// 守恒核：双恒等式 + 类型账合计。
    pub fn conserved(&self) -> Result<(), u16> {
        let acc = self.accepted();
        if acc != self.buf.len() as u32 + self.overflow_drop {
            return Err(XC_UNBALANCED);
        }
        let mut kc = 0u32;
        for i in 0..KIND_CAP {
            kc += self.kind_count[i];
        }
        if kc != acc {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 捕获摘要串：总入账/重放拒/过旧拒/溢出弃。
    pub fn summary(&self) -> String {
        let mut s = String::from("EC[");
        s.push_str(&self.total_in.to_string());
        s.push('/');
        s.push_str(&self.dup_reject.to_string());
        s.push('/');
        s.push_str(&self.stale_reject.to_string());
        s.push('/');
        s.push_str(&self.overflow_drop.to_string());
        s.push(']');
        s
    }

    /// 类型账只读。
    pub fn kind_stat(&self, kind: usize) -> Result<u32, u16> {
        if kind >= KIND_CAP {
            return Err(XC_BAD_KIND);
        }
        Ok(self.kind_count[kind])
    }
}

/// 自例程：开捕 → 顺序捕获 → 乱序补录 → 重放拒 → 过旧拒 →
/// 倒挂红码 → 冲刷升序 → 校验和复核 → 守恒 → 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut p = CapturePipe::new();
    // 错误路径 1：未开捕即捕获。
    if p.capture(1, K_NAV, 10) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    p.start()?;
    // 顺序捕获 10 条（seq 1..=10，kind 轮转）。
    for i in 1..=10u32 {
        let k = (i as usize) % KIND_CAP;
        p.capture(i, k, 10 * i)?;
    }
    // 乱序补录：先 12 后 11（11 ≤ 高水位 12 且 11+8>12 且全新）。
    p.capture(12, K_CUSTOM, 120)?;
    p.capture(11, K_LIFE, 115)?;
    // 错误路径 2：窗内重放（seq 5 已在库）。
    if p.capture(5, K_INPUT, 50) != Err(XC_DUP_SEQ) {
        return Err(XC_DUP_SEQ);
    }
    // 错误路径 3：过旧（seq 1：1+8=9 ≤ 高水位 12）。
    if p.capture(1, K_NAV, 10) != Err(XC_STALE_SEQ) {
        return Err(XC_STALE_SEQ);
    }
    // 错误路径 4：时间戳倒挂（120/115 之后回 100）。
    if p.capture(13, K_INPUT, 100) != Err(XC_TS_BACKWARD) {
        return Err(XC_TS_BACKWARD);
    }
    // 守恒 + 在库数核对（1..=12 全新 → 12 条）。
    p.conserved()?;
    if p.stored() != 12 {
        return Err(XC_UNBALANCED);
    }
    if p.high_water() != 12 {
        return Err(XC_UNBALANCED);
    }
    if p.kind_stat(K_NAV)? != 2 {
        return Err(XC_UNBALANCED);
    }
    // 冲刷：升序导出 + 逐条校验和复核。
    let out = p.flush()?;
    if out.len() != 12 {
        return Err(XC_UNBALANCED);
    }
    for i in 0..out.len() {
        if out[i].seq != (i + 1) as u32 {
            return Err(XC_UNBALANCED);
        }
        CapturePipe::verify(&out[i])?;
    }
    // 错误路径 5：Flushed 后禁捕获/再冲刷/重启。
    if p.capture(20, K_NAV, 200) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    if p.flush().is_err() {
        return Err(XC_STATE_BAD);
    }
    if p.start() != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 溢出弃：灌满 48 后第 49 条拒收。
    let mut p2 = CapturePipe::new();
    p2.start()?;
    for i in 1..=48u32 {
        p2.capture(i, K_NAV, i)?;
    }
    if p2.capture(49, K_NAV, 49) != Err(XC_PIPE_FULL) {
        return Err(XC_PIPE_FULL);
    }
    p2.conserved()?;
    // 摘要串核对：48 入/0 重放/0 过旧/1 溢出。
    let s = p2.summary();
    if !s.contains("48/0/0/1") {
        return Err(XC_UNBALANCED);
    }
    // 错误路径 6~8：坏序号/坏类型/坏时间戳。
    let mut p3 = CapturePipe::new();
    p3.start()?;
    if p3.capture(0, K_NAV, 1) != Err(XC_BAD_SEQ) {
        return Err(XC_BAD_SEQ);
    }
    if p3.capture(1, 9, 1) != Err(XC_BAD_KIND) {
        return Err(XC_BAD_KIND);
    }
    if p3.capture(1, K_NAV, TS_MAX + 1) != Err(XC_BAD_TS) {
        return Err(XC_BAD_TS);
    }
    // 错误路径 9：坏校验和记录复核。
    let bad = EvRecord {
        seq: 7,
        kind: K_NAV,
        ts: 70,
        hash: 0,
    };
    if CapturePipe::verify(&bad) != Err(XC_HASH_MISMATCH) {
        return Err(XC_HASH_MISMATCH);
    }
    // 错误路径 10：空管道冲刷。
    if p3.flush().err() != Some(XC_FLUSH_EMPTY) {
        return Err(XC_FLUSH_EMPTY);
    }
    Ok(0)
}
