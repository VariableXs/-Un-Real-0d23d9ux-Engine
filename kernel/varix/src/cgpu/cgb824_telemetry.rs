//! cgb824_telemetry — CGPU-F0824 Intel 遥测。
//!
//! 判据原文（验收行摘录）：Intel 遥测：事件环形账（定容环形缓冲，满则覆
//! 盖最旧并计溢出）；单调序列号（事件序号不可回退，乱序即拒）；通道配额
//! （按通道记账，超配额拒收——防单通道刷爆）；通道账守恒（事件总数 =
//! 各通道计数之和）。判据：环形账、序列号、配额、守恒。
//!
//! 诊断段 0x7180（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；环形账定容覆盖；通道账守恒每步可验；drill()
//! 自例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb824-v1";

/// 诊断段基（本单独占 0x7180）。
pub const SEG_BASE: u16 = 0x7180;

/// 遥测未启动（未 start 即投递）。
pub const XC_NOT_RUNNING: u16 = SEG_BASE + 1;
/// 序列号回退（事件序号 ≤ 上次受理序号）。
pub const XC_SEQ_BACKWARD: u16 = SEG_BASE + 2;
/// 通道未知（未注册的通道号）。
pub const XC_CHAN_UNKNOWN: u16 = SEG_BASE + 3;
/// 通道配额耗尽（单通道事件数超上限——防刷爆）。
pub const XC_QUOTA_EXCEEDED: u16 = SEG_BASE + 4;
/// 事件载荷过长（超出定容载荷上限）。
pub const XC_PAYLOAD_LONG: u16 = SEG_BASE + 5;
/// 通道账漂移（事件总数 ≠ 通道计数和——守恒破坏）。
pub const XC_CHAN_DRIFT: u16 = SEG_BASE + 6;

/// 环形账定容。
pub const RING_CAP: usize = 64;
/// 载荷上限（字节）。
pub const PAYLOAD_CAP: usize = 32;
/// 通道数上限。
pub const CHAN_CAP: usize = 8;
/// 单通道配额。
pub const CHAN_QUOTA: u64 = 20;

/// 遥测通道注册表（固定通道号 → 名称与用途）。
pub fn chan_name(chan: usize) -> &'static str {
    match chan {
        0 => "perf",
        1 => "power",
        2 => "mem",
        3 => "media",
        _ => "aux",
    }
}

/// 遥测事件。
#[derive(Clone, Debug)]
pub struct Event {
    /// 全局单调序列号。
    pub seq: u64,
    /// 通道号。
    pub chan: usize,
    /// 事件类别（0 信息/1 警告/2 故障）。
    pub kind: u8,
    /// 定容载荷（前 len 字节有效）。
    pub payload: [u8; PAYLOAD_CAP],
    /// 载荷有效长。
    pub plen: usize,
}

/// 通道账。
#[derive(Clone, Copy, Debug)]
pub struct ChanLedger {
    /// 受理计数。
    pub accepted: u64,
    /// 拒绝计数。
    pub rejected: u64,
}

impl ChanLedger {
    fn new() -> Self {
        ChanLedger { accepted: 0, rejected: 0 }
    }
}

/// 遥测收集器。
pub struct Telemetry {
    /// 收集器名。
    pub name: String,
    /// 运行位。
    running: bool,
    /// 环形账。
    ring: Vec<Event>,
    /// 写指针。
    wpos: usize,
    /// 覆盖次数（环形满后每次覆盖 +1——判据"满则覆盖并计溢出"）。
    pub overwrites: u64,
    /// 最近受理序列号。
    last_seq: u64,
    /// 通道账（定容 CHAN_CAP）。
    chans: Vec<ChanLedger>,
    /// 通道注册位。
    chan_reg: [bool; CHAN_CAP],
    /// 总受理。
    pub total: u64,
}

/// 守恒恒等式：total == Σ chans[i].accepted（i 遍历已注册通道）。
/// 每个 accept 成功后 verify() 复核，不等即 XC_CHAN_DRIFT。
impl Telemetry {
    /// 构造。
    pub fn new(name: &str) -> Self {
        let mut chans = Vec::new();
        for _ in 0..CHAN_CAP {
            chans.push(ChanLedger::new());
        }
        Telemetry {
            name: name.to_string(),
            running: false,
            ring: Vec::new(),
            wpos: 0,
            overwrites: 0,
            last_seq: 0,
            chans,
            chan_reg: [false; CHAN_CAP],
            total: 0,
        }
    }

    /// 启动遥测。
    pub fn start(&mut self) {
        self.running = true;
    }

    /// 注册通道。
    pub fn reg_chan(&mut self, chan: usize) -> Result<(), u16> {
        if chan >= CHAN_CAP {
            return Err(XC_CHAN_UNKNOWN);
        }
        self.chan_reg[chan] = true;
        Ok(())
    }

    /// 投递事件（判据"环形账+序列号+配额"三闸）。
    pub fn emit(&mut self, seq: u64, chan: usize, kind: u8, payload: &[u8]) -> Result<(), u16> {
        if !self.running {
            return Err(XC_NOT_RUNNING);
        }
        if chan >= CHAN_CAP || !self.chan_reg[chan] {
            return Err(XC_CHAN_UNKNOWN);
        }
        if seq <= self.last_seq {
            return Err(XC_SEQ_BACKWARD);
        }
        if payload.len() > PAYLOAD_CAP {
            return Err(XC_PAYLOAD_LONG);
        }
        if self.chans[chan].accepted >= CHAN_QUOTA {
            self.chans[chan].rejected += 1;
            return Err(XC_QUOTA_EXCEEDED);
        }
        let mut ev = Event { seq, chan, kind, payload: [0; PAYLOAD_CAP], plen: payload.len() };
        for (i, b) in payload.iter().enumerate() {
            ev.payload[i] = *b;
        }
        if self.ring.len() < RING_CAP {
            self.ring.push(ev);
        } else {
            self.ring[self.wpos] = ev;
            self.overwrites += 1;
        }
        self.wpos = (self.wpos + 1) % RING_CAP;
        self.last_seq = seq;
        self.chans[chan].accepted += 1;
        self.total += 1;
        self.verify()?;
        Ok(())
    }

    /// 最近受理序列号。
    pub fn seq_now(&self) -> u64 {
        self.last_seq
    }

    /// 按时序读取环形账（旧→新）。
    pub fn ordered(&self) -> Vec<Event> {
        let mut out = Vec::new();
        let n = self.ring.len();
        if n == 0 {
            return out;
        }
        let start = if n < RING_CAP { 0 } else { self.wpos };
        for i in 0..n {
            out.push(self.ring[(start + i) % RING_CAP]);
        }
        out
    }

    /// 通道受理计数。
    pub fn chan_accepted(&self, chan: usize) -> Result<u64, u16> {
        if chan >= CHAN_CAP {
            return Err(XC_CHAN_UNKNOWN);
        }
        Ok(self.chans[chan].accepted)
    }

    /// 通道拒绝计数。
    pub fn chan_rejected(&self, chan: usize) -> Result<u64, u16> {
        if chan >= CHAN_CAP {
            return Err(XC_CHAN_UNKNOWN);
        }
        Ok(self.chans[chan].rejected)
    }

    /// 守恒校验（判据"通道账守恒"）。
    pub fn verify(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        for i in 0..CHAN_CAP {
            if self.chan_reg[i] {
                sum += self.chans[i].accepted;
            }
        }
        if sum != self.total {
            return Err(XC_CHAN_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb824[");
        s.push_str(&self.name);
        s.push_str("] total=");
        s.push_str(&self.total.to_string());
        s.push_str(" ring=");
        s.push_str(&self.ring.len().to_string());
        s.push_str(" ow=");
        s.push_str(&self.overwrites.to_string());
        s.push_str(" seq=");
        s.push_str(&self.last_seq.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut t = Telemetry::new("w3-drill-824");

    // 负样本 1：未启动投递——应拒。
    match t.emit(1, 0, 0, b"early") {
        Err(code) if code == XC_NOT_RUNNING => {}
        Ok(_) => return Err(XC_NOT_RUNNING),
        Err(other) => return Err(other),
    }
    t.start();

    // 负样本 2：未注册通道——应拒。
    match t.emit(1, 3, 0, b"x") {
        Err(code) if code == XC_CHAN_UNKNOWN => {}
        Ok(_) => return Err(XC_CHAN_UNKNOWN),
        Err(other) => return Err(other),
    }
    t.reg_chan(0)?;
    t.reg_chan(1)?;
    t.reg_chan(2)?;

    // 负样本 3：载荷超长——应拒。
    let long = [7u8; PAYLOAD_CAP + 1];
    match t.emit(1, 0, 0, &long) {
        Err(code) if code == XC_PAYLOAD_LONG => {}
        Ok(_) => return Err(XC_PAYLOAD_LONG),
        Err(other) => return Err(other),
    }

    // 正样本：双通道交替投递。
    for i in 0..10u64 {
        t.emit(10 + i, (i as usize) % 2, if i == 5 { 1 } else { 0 }, b"ok")?;
    }
    if t.seq_now() != 19 {
        return Err(XC_SEQ_BACKWARD);
    }

    // 负样本 4：序列号回退——应拒。
    match t.emit(5, 0, 0, b"old") {
        Err(code) if code == XC_SEQ_BACKWARD => {}
        Ok(_) => return Err(XC_SEQ_BACKWARD),
        Err(other) => return Err(other),
    }
    // 负样本 5：重复序列号——应拒（单调）。
    match t.emit(19, 0, 0, b"dup") {
        Err(code) if code == XC_SEQ_BACKWARD => {}
        Ok(_) => return Err(XC_SEQ_BACKWARD),
        Err(other) => return Err(other),
    }

    // 配额：通道 0 已受理 5 次，补到配额上限再超额一次。
    let mut seq = 100u64;
    while t.chan_accepted(0)? < CHAN_QUOTA {
        t.emit(seq, 0, 0, b"fill")?;
        seq += 1;
    }
    match t.emit(seq, 0, 0, b"over") {
        Err(code) if code == XC_QUOTA_EXCEEDED => {}
        Ok(_) => return Err(XC_QUOTA_EXCEEDED),
        Err(other) => return Err(other),
    }
    if t.chan_rejected(0)? != 1 {
        return Err(XC_QUOTA_EXCEEDED);
    }

    // 环形覆盖：继续通道 1/2 投递直至覆盖发生。
    while t.overwrites == 0 {
        let ch = (1 + (seq % 2)) as usize;
        t.emit(seq, ch, 2, b"wrap")?;
        seq += 1;
    }
    if t.overwrites < 1 {
        return Err(XC_CHAN_DRIFT);
    }
    if t.ordered().len() != RING_CAP {
        return Err(XC_CHAN_DRIFT);
    }
    t.verify()?;
    // 守恒复核：total == 注册通道 accepted 之和。
    let sum = t.chan_accepted(0)? + t.chan_accepted(1)? + t.chan_accepted(2)?;
    if sum != t.total {
        return Err(XC_CHAN_DRIFT);
    }
    Ok(0)
}
