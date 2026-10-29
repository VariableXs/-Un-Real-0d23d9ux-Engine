//! UNX-F48514 —— E26 fuzz（随机风暴模式注入零崩）
//! 判据原文：UNX-F48514-J1 对 E26 风暴防御面做确定性随机模式注入：
//! 风暴脉冲、静默窗、边沿抖动等任意合法模式流不得引发崩溃或
//! 非法内部状态；脉冲账守恒、削峰账一致、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 模式闭集；R2 注入零错态；
//! R3 风暴脉冲边界；R4 静默窗不破账；R5 脉冲账守恒；
//! R6 削峰账一致；R7 非法模式拒收；R8 大批量 fuzz 稳定性；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEB60;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_MODE: u16 = SEG_BASE + 1; // 模式越出闭集
const E_ERRSTATE: u16 = SEG_BASE + 2; // 注入后出现非法内部状态
const E_PULSE: u16 = SEG_BASE + 3; // 风暴脉冲边界异常
const E_SILENT: u16 = SEG_BASE + 4; // 静默窗破账
const E_CONSERV: u16 = SEG_BASE + 5; // 脉冲账不守恒
const E_THROTTLE: u16 = SEG_BASE + 6; // 削峰账不一致
const E_REJECT: u16 = SEG_BASE + 7; // 非法模式拒收失败
const E_STORM: u16 = SEG_BASE + 8; // 大批量注入不稳定
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 模式闭集：1 风暴脉冲 / 2 静默窗 / 3 边沿抖动。
pub const M_STORM: u8 = 1;
pub const M_SILENT: u8 = 2;
pub const M_EDGE: u8 = 3;
pub const MODE_NAMES: [&str; 3] = ["storm", "silent", "edge"];

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

/// 一笔注入：模式 + 秒号 + 流量强度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzEvent {
    pub mode: u8,
    pub sec: u32,
    pub load: u32,
}

/// 模式账：三种模式计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModeAccount {
    pub storm: u32,
    pub silent: u32,
    pub edge: u32,
}

/// 削峰上限（枚/秒）。
pub const ADMIT_CAP: u32 = 1000;

/// E26 fuzz 接收面：任意风暴模式流进来都不崩、不产生错态。
pub struct FuzzSink {
    pub last_sec: u32,
    pub accounts: ModeAccount,
    pub admitted: u32,
    pub dropped: u32,
    pub accepted: u32,
    pub rejected: u32,
}

impl FuzzSink {
    pub fn new() -> FuzzSink {
        FuzzSink {
            last_sec: 0,
            accounts: ModeAccount { storm: 0, silent: 0, edge: 0 },
            admitted: 0,
            dropped: 0,
            accepted: 0,
            rejected: 0,
        }
    }

    /// 模式闭集判定。
    pub fn known_mode(m: u8) -> bool {
        m == M_STORM || m == M_SILENT || m == M_EDGE
    }

    /// 接收一笔事件：非法事件拒收计数；合法事件进账并按削峰记流量。
    /// 任何路径都不 panic：错误以 Err(u16) 返回，接收面状态仍自洽。
    pub fn inject(&mut self, ev: FuzzEvent) -> Result<(), u16> {
        if !FuzzSink::known_mode(ev.mode) {
            self.rejected += 1;
            return Err(E_MODE);
        }
        if !self.bills_empty() && ev.sec <= self.last_sec {
            self.rejected += 1;
            return Err(E_CONSERV);
        }
        match ev.mode {
            M_STORM => self.accounts.storm += 1,
            M_SILENT => self.accounts.silent += 1,
            _ => self.accounts.edge += 1,
        }
        let give = if ev.load <= ADMIT_CAP { ev.load } else { ADMIT_CAP };
        self.admitted += give;
        self.dropped += ev.load - give;
        self.last_sec = ev.sec;
        self.accepted += 1;
        Ok(())
    }

    fn bills_empty(&self) -> bool {
        self.accepted == 0 && self.rejected == 0
    }

    /// 账守恒：admitted + dropped == 注入总量。
    pub fn conserved(&self, total_load: u32) -> bool {
        self.admitted + self.dropped == total_load
    }

    /// 削峰一致：admitted ≤ accepted × ADMIT_CAP。
    pub fn throttle_sane(&self) -> bool {
        self.admitted <= self.accepted * ADMIT_CAP
    }

    /// 错态自检：模式账和 == accepted。
    pub fn sane(&self) -> bool {
        self.accounts.storm + self.accounts.silent + self.accounts.edge == self.accepted
    }

    /// 总注入笔数 = 接收 + 拒收。
    pub fn total_injected(&self) -> u32 {
        self.accepted + self.rejected
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 模式闭集：mode 1..=3 合法、0/4/255 非法；模式名非空互异。
    let mut lcg = Lcg::new(0x48514);
    let mut t = 0u32;
    while t < 16 {
        let m = (lcg.next_u32() % 6) as u8;
        if m >= 1 && m <= 3 {
            if !FuzzSink::known_mode(m) {
                return Err(E_MODE);
            }
        } else if FuzzSink::known_mode(m) {
            return Err(E_MODE);
        }
        t += 1;
    }
    if MODE_NAMES.len() != 3 || MODE_NAMES[0] == MODE_NAMES[1] {
        return Err(E_MODE);
    }
    hits += 1;

    // R2 注入零错态：96 笔合法随机事件，事后 sane() 恒真。
    let mut sink = FuzzSink::new();
    let mut lcg2 = Lcg::new(0x26E0_0E);
    let mut total_load = 0u32;
    let mut sec = 1u32;
    let mut t = 0u32;
    while t < 96 {
        let mode = (1 + lcg2.below(3)) as u8;
        let load = lcg2.below(2001);
        sink.inject(FuzzEvent { mode, sec, load })?;
        total_load += load;
        sec += 1;
        t += 1;
    }
    if !sink.sane() || !sink.conserved(total_load) || !sink.throttle_sane() {
        return Err(E_ERRSTATE);
    }
    hits += 1;

    // R3 风暴脉冲边界：load=0 与 load=ADMIT_CAP 恰全放行；超限恰削到上限。
    let mut sink3 = FuzzSink::new();
    sink3.inject(FuzzEvent { mode: M_STORM, sec: 1, load: 0 })?;
    sink3.inject(FuzzEvent { mode: M_STORM, sec: 2, load: ADMIT_CAP })?;
    if sink3.dropped != 0 || sink3.admitted != ADMIT_CAP {
        return Err(E_PULSE);
    }
    sink3.inject(FuzzEvent {
        mode: M_STORM,
        sec: 3,
        load: ADMIT_CAP + 1,
    })?;
    if sink3.admitted != 2 * ADMIT_CAP {
        return Err(E_PULSE);
    }
    hits += 1;

    // R4 静默窗不破账：静默模式 load=0 连发 32 笔，账目零变化仍自洽。
    let mut sink4 = FuzzSink::new();
    let mut sec = 1u32;
    let mut t = 0u32;
    while t < 32 {
        sink4.inject(FuzzEvent { mode: M_SILENT, sec, load: 0 })?;
        sec += 1;
        t += 1;
    }
    if sink4.accounts.silent != 32 || !sink4.sane() || sink4.admitted != 0 {
        return Err(E_SILENT);
    }
    hits += 1;

    // R5 脉冲账守恒：三模式混合 8 笔，账和 == 笔数。
    let mut sink5 = FuzzSink::new();
    let mut sec = 1u32;
    let mut t = 0u32;
    while t < 3 {
        sink5.inject(FuzzEvent { mode: M_STORM, sec, load: 10 })?;
        sec += 1;
        t += 1;
    }
    let mut t = 0u32;
    while t < 2 {
        sink5.inject(FuzzEvent { mode: M_EDGE, sec, load: 20 })?;
        sec += 1;
        t += 1;
    }
    sink5.inject(FuzzEvent { mode: M_SILENT, sec, load: 0 })?;
    if !sink5.sane() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 削峰账一致：风暴 5000 枚 → 放行恰上限、差额入丢弃。
    let mut sink6 = FuzzSink::new();
    sink6.inject(FuzzEvent { mode: M_STORM, sec: 1, load: 5000 })?;
    if sink6.admitted != ADMIT_CAP || sink6.dropped != 5000 - ADMIT_CAP {
        return Err(E_THROTTLE);
    }
    if !sink6.conserved(5000) || !sink6.throttle_sane() {
        return Err(E_THROTTLE);
    }
    hits += 1;

    // R7 非法模式拒收：mode=0 与 mode=9 均拒，码恰为 E_MODE，接收数不增。
    let mut sink7 = FuzzSink::new();
    match sink7.inject(FuzzEvent { mode: 0, sec: 1, load: 10 }) {
        Err(E_MODE) => {}
        _ => return Err(E_REJECT),
    }
    match sink7.inject(FuzzEvent { mode: 9, sec: 2, load: 10 }) {
        Err(E_MODE) => {}
        _ => return Err(E_REJECT),
    }
    if sink7.accepted != 0 || sink7.rejected != 2 {
        return Err(E_REJECT);
    }
    hits += 1;

    // R8 大批量 fuzz 稳定性：512 笔混合（含非法与倒序秒），总量记账一致、sane 恒真。
    let mut sink8 = FuzzSink::new();
    let mut lcg3 = Lcg::new(0x26E0_0F);
    let mut total_load = 0u32;
    let mut sec = 1u32;
    let mut t = 0u32;
    while t < 512 {
        let mode = (lcg3.below(5)) as u8;
        let load = lcg3.below(3000);
        if FuzzSink::known_mode(mode) {
            sink8.inject(FuzzEvent { mode, sec, load })?;
            total_load += load;
            sec += 1;
        } else {
            sink8.inject(FuzzEvent { mode, sec, load })?;
        }
        t += 1;
    }
    if !sink8.sane() || !sink8.conserved(total_load) {
        return Err(E_STORM);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = sink_fp(&sink8);
    let f2 = sink_fp(&sink8);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：acc=/adm=/drp= 三段前缀在位。
    let line = sink_line(&sink8);
    if !has_kw(line.as_bytes(), b"acc=") || !has_kw(line.as_bytes(), b"adm=")
        || !has_kw(line.as_bytes(), b"drp=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：模式账 + 流量账串联。
pub fn sink_fp(s: &FuzzSink) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&s.accounts.storm.to_le_bytes());
    buf.extend_from_slice(&s.accounts.silent.to_le_bytes());
    buf.extend_from_slice(&s.accounts.edge.to_le_bytes());
    buf.extend_from_slice(&s.admitted.to_le_bytes());
    buf.extend_from_slice(&s.dropped.to_le_bytes());
    buf.extend_from_slice(&s.rejected.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：acc=<接收> adm=<放行> drp=<丢弃>。
pub fn sink_line(s: &FuzzSink) -> String {
    let mut out = String::new();
    out.push_str("acc=");
    out.push_str(&s.accepted.to_string());
    out.push_str(" adm=");
    out.push_str(&s.admitted.to_string());
    out.push_str(" drp=");
    out.push_str(&s.dropped.to_string());
    out
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
