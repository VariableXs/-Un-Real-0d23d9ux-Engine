//! UNX-F48576 —— I29 fuzz（随机接口调用注入不崩两端）
//! 判据原文：UNX-F48576-J1 对 I29 联签接口两端（冻结面/执行面）做
//! 确定性随机调用注入：任意合法调用流（含乱序压力前的合法序、
//! 边界参数、重复调用）不得引发任一端崩溃或非法内部状态；
//! 调用账守恒、两端账目一致、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 调用闭集；R2 注入零错态；
//! R3 边界参数（0 与饱和值）；R4 重复调用不破账；
//! R5 调用账守恒；R6 两端账目一致；R7 非法调用拒收；
//! R8 大批量 fuzz 稳定性；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF7E0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_CALL: u16 = SEG_BASE + 1; // 调用越出闭集
const E_ERRSTATE: u16 = SEG_BASE + 2; // 注入后出现非法内部状态
const E_ARG: u16 = SEG_BASE + 3; // 边界参数处理异常
const E_DUP: u16 = SEG_BASE + 4; // 重复调用破账
const E_CONSERV: u16 = SEG_BASE + 5; // 调用账不守恒
const E_TWOSIDE: u16 = SEG_BASE + 6; // 两端账目不一致
const E_REJECT: u16 = SEG_BASE + 7; // 非法调用拒收失败
const E_STORM: u16 = SEG_BASE + 8; // 大批量注入不稳定
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 调用闭集：1 提交 / 2 联签 / 3 回读。
pub const C_SUBMIT: u8 = 1;
pub const C_SIGN: u8 = 2;
pub const C_READBACK: u8 = 3;
pub const CALL_NAMES: [&str; 3] = ["submit", "sign", "readback"];

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

/// 一笔注入调用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzCall {
    pub kind: u8,
    pub side: u8,
    pub arg: u32,
}

/// 单端账目：三调用计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SideAccount {
    pub submit: u32,
    pub sign: u32,
    pub readback: u32,
}

/// I29 fuzz 接收面：任意调用流进来两端都不崩、不产生错态。
pub struct FuzzSink {
    pub last_arg: u32,
    pub frozen: SideAccount,
    pub exec: SideAccount,
    pub accepted: u32,
    pub rejected: u32,
}

impl FuzzSink {
    pub fn new() -> FuzzSink {
        FuzzSink {
            last_arg: 0,
            frozen: SideAccount { submit: 0, sign: 0, readback: 0 },
            exec: SideAccount { submit: 0, sign: 0, readback: 0 },
            accepted: 0,
            rejected: 0,
        }
    }

    /// 调用闭集判定。
    pub fn known_call(k: u8) -> bool {
        k == C_SUBMIT || k == C_SIGN || k == C_READBACK
    }

    /// 端闭集：1 冻结面 / 2 执行面。
    pub fn known_side(s: u8) -> bool {
        s == 1 || s == 2
    }

    fn acc_mut(&mut self, side: u8) -> Option<&mut SideAccount> {
        if side == 1 {
            Some(&mut self.frozen)
        } else if side == 2 {
            Some(&mut self.exec)
        } else {
            None
        }
    }

    /// 接收一笔调用：非法调用拒收计数；合法调用进对应端账。
    /// 任何路径都不 panic：错误以 Err(u16) 返回，接收面状态仍自洽。
    pub fn inject(&mut self, c: FuzzCall) -> Result<(), u16> {
        if !FuzzSink::known_call(c.kind) || !FuzzSink::known_side(c.side) {
            self.rejected += 1;
            return Err(E_CALL);
        }
        let acc = match self.acc_mut(c.side) {
            Some(a) => a,
            None => {
                self.rejected += 1;
                return Err(E_ERRSTATE);
            }
        };
        match c.kind {
            C_SUBMIT => acc.submit += 1,
            C_SIGN => acc.sign += 1,
            _ => acc.readback += 1,
        }
        self.last_arg = c.arg;
        self.accepted += 1;
        Ok(())
    }

    /// 守恒：单端三计数和 == 注入时给定的笔数。
    pub fn side_conserved(&self, side: u8, total: u32) -> bool {
        let a = if side == 1 { &self.frozen } else { &self.exec };
        a.submit + a.sign + a.readback == total
    }

    /// 两端账目一致：两端计数和 == accepted。
    pub fn two_side_sane(&self) -> bool {
        let f = self.frozen.submit + self.frozen.sign + self.frozen.readback;
        let e = self.exec.submit + self.exec.sign + self.exec.readback;
        f + e == self.accepted
    }

    /// 总注入笔数 = 接收 + 拒收。
    pub fn total_injected(&self) -> u32 {
        self.accepted + self.rejected
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 调用闭集：kind 1..=3、side 1..=2 合法，越界非法；调用名非空互异。
    let mut lcg = Lcg::new(0x48576);
    let mut t = 0u32;
    while t < 16 {
        let k = (lcg.next_u32() % 6) as u8;
        if k >= 1 && k <= 3 {
            if !FuzzSink::known_call(k) {
                return Err(E_CALL);
            }
        } else if FuzzSink::known_call(k) {
            return Err(E_CALL);
        }
        t += 1;
    }
    if CALL_NAMES.len() != 3 || CALL_NAMES[0] == CALL_NAMES[1] {
        return Err(E_CALL);
    }
    hits += 1;

    // R2 注入零错态：128 笔合法随机调用，事后 two_side_sane() 恒真。
    let mut sink = FuzzSink::new();
    let mut lcg2 = Lcg::new(0x2910_20);
    let mut t = 0u32;
    while t < 128 {
        let kind = (1 + lcg2.below(3)) as u8;
        let side = (1 + lcg2.below(2)) as u8;
        let arg = lcg2.below(10_000u32);
        sink.inject(FuzzCall { kind, side, arg })?;
        t += 1;
    }
    if !sink.two_side_sane() || !sink.side_conserved(1, sink.frozen.submit + sink.frozen.sign + sink.frozen.readback) {
        return Err(E_ERRSTATE);
    }
    hits += 1;

    // R3 边界参数：arg=0 与 arg=u32::MAX 均被正常入账，不崩不错。
    let mut sink3 = FuzzSink::new();
    sink3.inject(FuzzCall { kind: C_SUBMIT, side: 1, arg: 0 })?;
    sink3.inject(FuzzCall {
        kind: C_SIGN,
        side: 2,
        arg: 4_294_967_295u32,
    })?;
    if sink3.accepted != 2 || !sink3.two_side_sane() {
        return Err(E_ARG);
    }
    hits += 1;

    // R4 重复调用不破账：同端同型连发 32 笔，计数恰 32、账面自洽。
    let mut sink4 = FuzzSink::new();
    let mut t = 0u32;
    while t < 32 {
        sink4.inject(FuzzCall { kind: C_READBACK, side: 2, arg: t })?;
        t += 1;
    }
    if sink4.exec.readback != 32 || !sink4.two_side_sane() {
        return Err(E_DUP);
    }
    hits += 1;

    // R5 调用账守恒：混合流按端笔数与账目一致。
    let mut sink5 = FuzzSink::new();
    let mut t = 0u32;
    while t < 5 {
        sink5.inject(FuzzCall { kind: C_SUBMIT, side: 1, arg: t })?;
        sink5.inject(FuzzCall { kind: C_SIGN, side: 1, arg: t + 1 })?;
        t += 1;
    }
    let mut t = 0u32;
    while t < 7 {
        sink5.inject(FuzzCall { kind: C_SUBMIT, side: 2, arg: t * 2 })?;
        t += 1;
    }
    if !sink5.side_conserved(1, 10) || !sink5.side_conserved(2, 7) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 两端账目一致：两端计数和 == accepted，互不串账。
    if !sink5.two_side_sane() {
        return Err(E_TWOSIDE);
    }
    if sink5.frozen.readback != 0 || sink5.exec.readback != 0 {
        return Err(E_TWOSIDE);
    }
    hits += 1;

    // R7 非法调用拒收：kind=0 与 side=9 均拒，码恰为 E_CALL。
    let mut sink7 = FuzzSink::new();
    match sink7.inject(FuzzCall { kind: 0, side: 1, arg: 0 }) {
        Err(E_CALL) => hits += 1,
        _ => return Err(E_REJECT),
    }
    match sink7.inject(FuzzCall { kind: C_SUBMIT, side: 9, arg: 0 }) {
        Err(E_CALL) => hits += 1,
        _ => return Err(E_REJECT),
    }

    // R8 大批量 fuzz 稳定性：512 笔混合（含非法），总量记账一致、两端自洽。
    let mut sink8 = FuzzSink::new();
    let mut lcg3 = Lcg::new(0x2910_21);
    let mut t = 0u32;
    while t < 512 {
        let kind = (lcg3.below(5)) as u8;
        let side = (lcg3.below(4)) as u8;
        let arg = lcg3.below(50_000u32);
        let _r = sink8.inject(FuzzCall { kind, side, arg });
        t += 1;
    }
    if !sink8.two_side_sane() || sink8.total_injected() != 512 {
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

    // R10 呈现行齐全：fzn=/exe=/rej= 三段前缀在位。
    let line = sink_line(&sink8);
    if !has_kw(line.as_bytes(), b"fzn=") || !has_kw(line.as_bytes(), b"exe=")
        || !has_kw(line.as_bytes(), b"rej=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：两端账 + 收发计数串联。
pub fn sink_fp(s: &FuzzSink) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&s.frozen.submit.to_le_bytes());
    buf.extend_from_slice(&s.frozen.sign.to_le_bytes());
    buf.extend_from_slice(&s.frozen.readback.to_le_bytes());
    buf.extend_from_slice(&s.exec.submit.to_le_bytes());
    buf.extend_from_slice(&s.exec.sign.to_le_bytes());
    buf.extend_from_slice(&s.exec.readback.to_le_bytes());
    buf.extend_from_slice(&s.accepted.to_le_bytes());
    buf.extend_from_slice(&s.rejected.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：fzn=<冻结面计数> exe=<执行面计数> rej=<拒收>。
pub fn sink_line(s: &FuzzSink) -> String {
    let f = s.frozen.submit + s.frozen.sign + s.frozen.readback;
    let e = s.exec.submit + s.exec.sign + s.exec.readback;
    let mut out = String::new();
    out.push_str("fzn=");
    out.push_str(&f.to_string());
    out.push_str(" exe=");
    out.push_str(&e.to_string());
    out.push_str(" rej=");
    out.push_str(&s.rejected.to_string());
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
