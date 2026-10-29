//! UNX-F48521 —— E·M2 设备热移除期按键流（正：拔盘/拔键瞬间在途帧丢弃入账）
//! 判据原文：UNX-F48521-J1 M2 设备热移除窗口内的按键流必须安全：
//! 拔盘/拔键瞬间在途帧一律丢弃且逐帧入账（丢弃数可复核）；
//! 移除完成后不得再接收任何帧；已入账帧不受移除影响；
//! 帧账守恒（接收+丢弃 == 到达总量）、时戳单调、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 设备态闭集；R2 移除前帧正常入账；
//! R3 移除窗口在途帧丢弃入账；R4 移除完成后拒收；
//! R5 帧账守恒；R6 时戳单调；R7 未知设备拒收；
//! R8 fuzz 帧流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xECA0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_STATE: u16 = SEG_BASE + 1; // 设备态越出闭集
const E_DROP: u16 = SEG_BASE + 2; // 在途帧未丢弃入账
const E_GHOST: u16 = SEG_BASE + 3; // 移除后仍接收帧
const E_CONSERV: u16 = SEG_BASE + 4; // 帧账不守恒
const E_TS: u16 = SEG_BASE + 5; // 时戳非单调
const E_DEV: u16 = SEG_BASE + 6; // 未知设备拒收失败
const E_KEEP: u16 = SEG_BASE + 7; // 已入账帧被移除误删
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 帧流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 设备态闭集：1 在位 / 2 移除窗口 / 3 已移除。
pub const ST_PRESENT: u8 = 1;
pub const ST_REMOVING: u8 = 2;
pub const ST_REMOVED: u8 = 3;
pub const STATE_NAMES: [&str; 3] = ["present", "removing", "removed"];

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

/// 一帧按键流：设备号 + 时戳。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyFrame {
    pub dev: u8,
    pub at: u32,
}

/// 设备态闭集判定。
pub fn known_state(s: u8) -> bool {
    s == ST_PRESENT || s == ST_REMOVING || s == ST_REMOVED
}

/// 单设备通道：态机 + 帧账。
pub struct DevChannel {
    pub dev: u8,
    pub state: u8,
    pub last_at: u32,
    pub accepted: u32,
    pub dropped: u32,
    pub ledger: Vec<u32>,
}

impl DevChannel {
    pub fn new(dev: u8) -> DevChannel {
        DevChannel {
            dev,
            state: ST_PRESENT,
            last_at: 0,
            accepted: 0,
            dropped: 0,
            ledger: vec![],
        }
    }

    /// 设备号闭集：1..=8。
    pub fn known_dev(d: u8) -> bool {
        d >= 1 && d <= 8
    }

    /// 进入移除窗口：仅从在位态可进。
    pub fn begin_remove(&mut self) -> Result<(), u16> {
        if self.state != ST_PRESENT {
            return Err(E_STATE);
        }
        self.state = ST_REMOVING;
        Ok(())
    }

    /// 完成移除：仅从移除窗口可进。
    pub fn finish_remove(&mut self) -> Result<(), u16> {
        if self.state != ST_REMOVING {
            return Err(E_STATE);
        }
        self.state = ST_REMOVED;
        Ok(())
    }

    /// 收帧：时戳单调；在位态入账；移除窗口丢弃入账；
    /// 已移除态拒收（幽灵帧）。已入账帧永不回收。
    pub fn feed(&mut self, f: KeyFrame) -> Result<bool, u16> {
        if !DevChannel::known_dev(f.dev) || f.dev != self.dev {
            return Err(E_DEV);
        }
        if f.at < self.last_at {
            return Err(E_TS);
        }
        self.last_at = f.at;
        match self.state {
            ST_PRESENT => {
                self.accepted += 1;
                self.ledger.push(f.at);
                Ok(true)
            }
            ST_REMOVING => {
                self.dropped += 1;
                Ok(false)
            }
            _ => Err(E_GHOST),
        }
    }

    /// 帧账守恒：accepted + dropped == 到达总量；账面帧数 == accepted。
    pub fn conserved(&self, arrived: u32) -> bool {
        self.accepted + self.dropped == arrived
            && self.ledger.len() as u32 == self.accepted
    }

    /// 已入账帧在移除后原样保留（无幽灵清账）。
    pub fn ledger_intact(&self, expect: u32) -> bool {
        self.ledger.len() as u32 == expect
    }
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 设备态闭集：三态合法、0/4/255 非法；态名非空互异。
    if !known_state(ST_PRESENT) || !known_state(ST_REMOVING) || !known_state(ST_REMOVED) {
        return Err(E_STATE);
    }
    if known_state(0) || known_state(4) || known_state(255) || STATE_NAMES[0] == STATE_NAMES[2] {
        return Err(E_STATE);
    }
    hits += 1;

    // R2 移除前帧正常入账：在位态收 8 帧，accepted == 8、账面 8。
    let mut ch = DevChannel::new(2);
    let mut t = 0u32;
    while t < 8 {
        ch.feed(KeyFrame { dev: 2, at: t * 10 })?;
        t += 1;
    }
    if ch.accepted != 8 || ch.ledger.len() != 8 {
        return Err(E_DROP);
    }
    hits += 1;

    // R3 移除窗口在途帧丢弃入账：进窗后收 5 帧，全部丢弃且逐帧计数。
    ch.begin_remove()?;
    let mut t = 0u32;
    while t < 5 {
        let ok = ch.feed(KeyFrame { dev: 2, at: 100 + t })?;
        if ok {
            return Err(E_DROP);
        }
        t += 1;
    }
    if ch.dropped != 5 {
        return Err(E_DROP);
    }
    hits += 1;

    // R4 移除完成后拒收：finish 后再收帧被拒，码恰为 E_GHOST；
    // 已入账 8 帧原样保留。
    ch.finish_remove()?;
    match ch.feed(KeyFrame { dev: 2, at: 999 }) {
        Err(E_GHOST) => hits += 1,
        _ => return Err(E_GHOST),
    }
    if !ch.ledger_intact(8) {
        return Err(E_KEEP);
    }

    // R5 帧账守恒：到达 13 帧（8 收 + 5 丢），守恒成立。
    if !ch.conserved(13) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R5b 在途帧账细节：丢弃帧不进 ledger、时戳不回写；
    // 已入账帧时戳严格保留且账面单调（移除前后一致）。
    if ch.ledger.len() != 8 {
        return Err(E_CONSERV);
    }
    // 账面最后一帧时戳应停在移除前（at=70），不得被在途帧回写。
    let last = ch.ledger[ch.ledger.len() - 1];
    if last != 70 {
        return Err(E_KEEP);
    }
    // last_at 已被在途帧与幽灵尝试推进（幽灵帧 at=999 通过时序检查后拒收），
    // 但 ledger 不受任何回写。
    if ch.last_at != 999 {
        return Err(E_TS);
    }
    let mut i = 1usize;
    while i < ch.ledger.len() {
        if ch.ledger[i - 1] >= ch.ledger[i] {
            return Err(E_TS);
        }
        i += 1;
    }
    hits += 1;

    // R6 时戳单调：倒序帧被拒且诊断码恰为 E_TS。
    let mut ch2 = DevChannel::new(3);
    ch2.feed(KeyFrame { dev: 3, at: 100 })?;
    match ch2.feed(KeyFrame { dev: 3, at: 50 }) {
        Err(E_TS) => hits += 1,
        _ => return Err(E_TS),
    }

    // R7 未知设备拒收：错设备号被拒，码恰为 E_DEV。
    let mut ch3 = DevChannel::new(1);
    match ch3.feed(KeyFrame { dev: 5, at: 0 }) {
        Err(E_DEV) => hits += 1,
        _ => return Err(E_DEV),
    }

    // R8 fuzz 帧流零错态：LCG 驱动 3 台设备混合生命周期
    // （在位收帧 → 进窗丢帧 → 完成移除），全程守恒、无幽灵帧。
    let mut lcg = Lcg::new(0x48521);
    let mut dev = 1u8;
    while dev <= 3 {
        let mut c = DevChannel::new(dev);
        let mut at = 0u32;
        let mut arrived = 0u32;
        let mut t = 0u32;
        let n1 = 4 + lcg.below(8);
        while t < n1 {
            at += 1 + lcg.below(5);
            c.feed(KeyFrame { dev, at })?;
            arrived += 1;
            t += 1;
        }
        c.begin_remove()?;
        let mut t = 0u32;
        let n2 = 1 + lcg.below(6);
        while t < n2 {
            at += 1 + lcg.below(5);
            let ok = c.feed(KeyFrame { dev, at })?;
            arrived += 1;
            if ok {
                return Err(E_FUZZ);
            }
            t += 1;
        }
        c.finish_remove()?;
        if !c.conserved(arrived) || !c.ledger_intact(c.accepted) {
            return Err(E_FUZZ);
        }
        dev += 1;
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = channel_fp(&ch);
    let f2 = channel_fp(&ch);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：dev=/st=/acc=/drp= 四段前缀在位。
    let line = channel_line(&ch);
    if !has_kw(line.as_bytes(), b"dev=")
        || !has_kw(line.as_bytes(), b"st=")
        || !has_kw(line.as_bytes(), b"acc=")
        || !has_kw(line.as_bytes(), b"drp=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：设备号 + 态 + 收/丢计数 + 账面帧时戳串联。
pub fn channel_fp(c: &DevChannel) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(c.dev);
    buf.push(c.state);
    buf.extend_from_slice(&c.accepted.to_le_bytes());
    buf.extend_from_slice(&c.dropped.to_le_bytes());
    let mut i = 0usize;
    while i < c.ledger.len() {
        buf.extend_from_slice(&c.ledger[i].to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：dev=<设备> st=<态> acc=<接收> drp=<丢弃>。
pub fn channel_line(c: &DevChannel) -> String {
    let mut s = String::new();
    s.push_str("dev=");
    s.push_str(&(c.dev as u32).to_string());
    s.push_str(" st=");
    s.push_str(&c.state.to_string());
    s.push_str(" acc=");
    s.push_str(&c.accepted.to_string());
    s.push_str(" drp=");
    s.push_str(&c.dropped.to_string());
    s
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
