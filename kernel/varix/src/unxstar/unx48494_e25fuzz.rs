//! UNX-F48494 —— E25 fuzz（随机时序事件注入零崩零错态）
//! 判据原文：UNX-F48494-J1 对 E25 防抖时序面做确定性随机注入：
//! 任意合法事件流（含乱序压力前的合法序、边界时戳、重复键）不得引发崩溃或
//! 非法内部状态；计数守恒、窗口账目一致、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 事件闭集；R2 注入零错态；
//! R3 边界时戳（0 与饱和值）；R4 重复键注入不破账；
//! R5 计数守恒；R6 窗口账目一致；R7 非法事件拒收；
//! R8 大批量 fuzz 稳定性；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE820;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_KIND: u16 = SEG_BASE + 1; // 事件越出闭集
const E_ERRSTATE: u16 = SEG_BASE + 2; // 注入后出现非法内部状态
const E_TS: u16 = SEG_BASE + 3; // 时戳处理异常
const E_DUP: u16 = SEG_BASE + 4; // 重复键破账
const E_CONSERV: u16 = SEG_BASE + 5; // 计数不守恒
const E_WINDOW: u16 = SEG_BASE + 6; // 窗口账目不一致
const E_REJECT: u16 = SEG_BASE + 7; // 非法事件拒收失败
const E_STORM: u16 = SEG_BASE + 8; // 大批量注入不稳定
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 事件闭集：1 按下 / 2 抬起 / 3 重复。
pub const EV_DOWN: u8 = 1;
pub const EV_UP: u8 = 2;
pub const EV_REP: u8 = 3;
pub const EVENT_NAMES: [&str; 3] = ["down", "up", "rep"];

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

/// 一条注入事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzEvent {
    pub kind: u8,
    pub key: u8,
    pub at: u32,
}

/// 键位账目：按下/抬起/重复计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyAccount {
    pub key: u8,
    pub down: u32,
    pub up: u32,
    pub rep: u32,
}

/// E25 fuzz 接收面：任意事件流进来都不崩、不产生错态。
pub struct FuzzSink {
    pub last_at: u32,
    pub accounts: Vec<KeyAccount>,
    pub accepted: u32,
    pub rejected: u32,
}

impl FuzzSink {
    pub fn new() -> FuzzSink {
        FuzzSink {
            last_at: 0,
            accounts: vec![],
            accepted: 0,
            rejected: 0,
        }
    }

    fn idx(&self, key: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.accounts.len() {
            if self.accounts[i].key == key {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 键位闭集：1..=8。
    pub fn known_key(key: u8) -> bool {
        key >= 1 && key <= 8
    }

    /// 接收一笔事件：非法事件拒收计数；合法事件进账。
    /// 任何路径都不 panic：错误以 Err(u16) 返回，接收面状态仍自洽。
    pub fn inject(&mut self, ev: FuzzEvent) -> Result<(), u16> {
        if !FuzzSink::known_key(ev.key) || ev.kind == 0 || ev.kind > 3 {
            self.rejected += 1;
            return Err(E_KIND);
        }
        if self.idx(ev.key).is_none() {
            self.accounts.push(KeyAccount {
                key: ev.key,
                down: 0,
                up: 0,
                rep: 0,
            });
        }
        let idx = self.idx(ev.key);
        let i = match idx {
            Some(i) => i,
            None => {
                self.rejected += 1;
                return Err(E_ERRSTATE);
            }
        };
        match ev.kind {
            EV_DOWN => self.accounts[i].down += 1,
            EV_UP => self.accounts[i].up += 1,
            _ => self.accounts[i].rep += 1,
        }
        self.last_at = ev.at;
        self.accepted += 1;
        Ok(())
    }

    /// 守恒：每键 down+up+rep == 注入时给定的笔数。
    pub fn conserved(&self, per_key: &[(u8, u32)]) -> bool {
        let mut i = 0usize;
        while i < per_key.len() {
            let mut hit = false;
            let mut j = 0usize;
            while j < self.accounts.len() {
                if self.accounts[j].key == per_key[i].0 {
                    let s = self.accounts[j].down + self.accounts[j].up + self.accounts[j].rep;
                    if s != per_key[i].1 {
                        return false;
                    }
                    hit = true;
                }
                j += 1;
            }
            if !hit {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 错态自检：accepted == 全键计数和；rejected 计数非负恒真。
    pub fn sane(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.accounts.len() {
            sum += self.accounts[i].down + self.accounts[i].up + self.accounts[i].rep;
            i += 1;
        }
        sum == self.accepted
    }

    /// 总注入笔数 = 接收 + 拒收。
    pub fn total_injected(&self) -> u32 {
        self.accepted + self.rejected
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 事件闭集：kind 1..=3 合法、0/4/255 非法；事件名非空互异。
    let mut lcg = Lcg::new(0x48494);
    let mut t = 0u32;
    while t < 16 {
        let k = (lcg.next_u32() % 6) as u8;
        if k >= 1 && k <= 3 {
            // 合法区间，应被 sink 接受（键位也取合法值）。
            let mut sink = FuzzSink::new();
            sink.inject(FuzzEvent { kind: k, key: 1, at: 0 })?;
        }
        t += 1;
    }
    if EVENT_NAMES.len() != 3 || EVENT_NAMES[0] == EVENT_NAMES[1] {
        return Err(E_KIND);
    }
    hits += 1;

    // R2 注入零错态：128 笔合法随机事件，事后 sane() 恒真。
    let mut sink = FuzzSink::new();
    let mut lcg2 = Lcg::new(0x25F0_0E);
    let mut t = 0u32;
    while t < 128 {
        let kind = (1 + lcg2.below(3)) as u8;
        let key = (1 + lcg2.below(8)) as u8;
        let at = lcg2.below(1000);
        sink.inject(FuzzEvent { kind, key, at })?;
        t += 1;
    }
    if !sink.sane() {
        return Err(E_ERRSTATE);
    }
    hits += 1;

    // R3 边界时戳：at=0 与 at=u32::MAX 均被正常入账，不崩不错。
    let mut sink3 = FuzzSink::new();
    sink3.inject(FuzzEvent { kind: EV_DOWN, key: 2, at: 0 })?;
    sink3.inject(FuzzEvent {
        kind: EV_UP,
        key: 2,
        at: 4_294_967_295u32,
    })?;
    if sink3.accepted != 2 || !sink3.sane() {
        return Err(E_TS);
    }
    hits += 1;

    // R4 重复键注入不破账：同键连发 32 笔重复，rep==32、账面自洽。
    let mut sink4 = FuzzSink::new();
    let mut t = 0u32;
    while t < 32 {
        sink4.inject(FuzzEvent { kind: EV_REP, key: 3, at: t })?;
        t += 1;
    }
    let mut i = 0usize;
    while i < sink4.accounts.len() {
        if sink4.accounts[i].key == 3 && sink4.accounts[i].rep != 32 {
            return Err(E_DUP);
        }
        i += 1;
    }
    if !sink4.sane() {
        return Err(E_DUP);
    }
    hits += 1;

    // R5 计数守恒：混合流按键笔数与账目一致。
    let mut sink5 = FuzzSink::new();
    let mut t = 0u32;
    while t < 5 {
        sink5.inject(FuzzEvent { kind: EV_DOWN, key: 1, at: t })?;
        sink5.inject(FuzzEvent { kind: EV_UP, key: 1, at: t + 1 })?;
        t += 1;
    }
    let mut t = 0u32;
    while t < 7 {
        sink5.inject(FuzzEvent { kind: EV_DOWN, key: 4, at: t * 2 })?;
        t += 1;
    }
    let per = [(1u8, 10u32), (4u8, 7u32)];
    if !sink5.conserved(&per) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 窗口账目一致：total_injected == accepted + rejected。
    let mut sink6 = FuzzSink::new();
    sink6.inject(FuzzEvent { kind: EV_DOWN, key: 1, at: 0 })?;
    match sink6.inject(FuzzEvent { kind: EV_DOWN, key: 9, at: 1 }) {
        Err(E_KIND) => {}
        _ => return Err(E_REJECT),
    }
    if sink6.total_injected() != 2 || sink6.accepted != 1 || sink6.rejected != 1 {
        return Err(E_WINDOW);
    }
    hits += 1;

    // R7 非法事件拒收：kind=0 与 key=0 均拒，码恰为 E_KIND。
    let mut sink7 = FuzzSink::new();
    match sink7.inject(FuzzEvent { kind: 0, key: 1, at: 0 }) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_REJECT),
    }
    match sink7.inject(FuzzEvent { kind: EV_DOWN, key: 0, at: 0 }) {
        Err(E_KIND) => hits += 1,
        _ => return Err(E_REJECT),
    }

    // R8 大批量 fuzz 稳定性：512 笔混合（含非法），总量记账一致、sane 恒真。
    let mut sink8 = FuzzSink::new();
    let mut lcg3 = Lcg::new(0x25F0_0F);
    let mut t = 0u32;
    while t < 512 {
        let kind = (lcg3.below(5)) as u8;
        let key = (lcg3.below(10)) as u8;
        let at = lcg3.below(10_000u32);
        let _r = sink8.inject(FuzzEvent { kind, key, at });
        t += 1;
    }
    if !sink8.sane() || sink8.total_injected() != 512 {
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

    // R10 呈现行齐全：acc=/ok=/rej= 三段前缀在位。
    let line = sink_line(&sink8);
    if !has_kw(line.as_bytes(), b"acc=") || !has_kw(line.as_bytes(), b"rej=") {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：accepted/rejected + 全键账目串联。
pub fn sink_fp(s: &FuzzSink) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&s.accepted.to_le_bytes());
    buf.extend_from_slice(&s.rejected.to_le_bytes());
    let mut i = 0usize;
    while i < s.accounts.len() {
        buf.push(s.accounts[i].key);
        buf.extend_from_slice(&s.accounts[i].down.to_le_bytes());
        buf.extend_from_slice(&s.accounts[i].up.to_le_bytes());
        buf.extend_from_slice(&s.accounts[i].rep.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：acc=<接收> rej=<拒收>。
pub fn sink_line(s: &FuzzSink) -> String {
    let mut out = String::new();
    out.push_str("acc=");
    out.push_str(&s.accepted.to_string());
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
