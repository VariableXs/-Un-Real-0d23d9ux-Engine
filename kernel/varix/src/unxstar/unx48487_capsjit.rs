//! UNX-F48487 —— E·CapsLock 抖动（正：快速连按状态恒定）
//! 判据原文：UNX-F48487-J1 CapsLock 快速连按（按下间隔小于抖动消除窗）
//! 必须被视为机械抖动：状态恒定不翻转；只有间隔 ≥ 消除窗的按下
//! 才允许翻转一次；抖动事件全额记账、守恒、指纹可复核。
//! 判据落地（drill 场景）：R1 消除窗边界（窗内不翻、窗外翻一次）；
//! R2 快速连按状态恒定；R3 慢速连按逐次翻转；R4 抖动记账守恒；
//! R5 混合流（快慢交替）账目一致；R6 状态机无非法转移；
//! R7 复位归零；R8 指纹稳定；R9 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE700;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_FLIP: u16 = SEG_BASE + 1; // 窗内事件发生了翻转（状态漂移）
const E_BOUND: u16 = SEG_BASE + 2; // 消除窗边界判定错误
const E_CONSERV: u16 = SEG_BASE + 3; // 事件记账不守恒
const E_STATE: u16 = SEG_BASE + 4; // 状态机非法转移
const E_RESET: u16 = SEG_BASE + 5; // 复位归零失败
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_SLOW: u16 = SEG_BASE + 8; // 慢速按下未翻转

/// 抖动消除窗（毫秒）：间隔小于此值的按下视为抖动。
pub const DEBOUNCE_MS: u32 = 80;

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

/// CapsLock 状态机：锁定态 + 抖动消除窗。
pub struct CapsMachine {
    pub locked: bool,
    pub last_flip_at: Option<u32>,
    pub flips: u32,
    pub jitter_events: u32,
    pub presses: u32,
}

impl CapsMachine {
    pub fn new() -> CapsMachine {
        CapsMachine {
            locked: false,
            last_flip_at: None,
            flips: 0,
            jitter_events: 0,
            presses: 0,
        }
    }

    /// 灌入一次按下（at 为时戳）：
    /// 首按必翻；此后间隔 < 消除窗 → 抖动（不翻）；间隔 ≥ 窗 → 翻转一次。
    pub fn press(&mut self, at: u32) -> Result<bool, u16> {
        self.presses += 1;
        match self.last_flip_at {
            None => {
                self.locked = !self.locked;
                self.flips += 1;
                self.last_flip_at = Some(at);
                Ok(true)
            }
            Some(prev) => {
                if at.saturating_sub(prev) < DEBOUNCE_MS {
                    // 抖动：状态恒定，只记账。
                    self.jitter_events += 1;
                    Ok(false)
                } else {
                    self.locked = !self.locked;
                    self.flips += 1;
                    self.last_flip_at = Some(at);
                    Ok(true)
                }
            }
        }
    }

    /// 记账守恒：flips + jitter_events == presses。
    pub fn conserved(&self) -> bool {
        self.flips + self.jitter_events == self.presses
    }

    /// 状态一致性：flips 为偶数 ⇒ 未锁定；奇数 ⇒ 锁定。
    pub fn state_consistent(&self) -> bool {
        if self.flips % 2 == 0 {
            !self.locked
        } else {
            self.locked
        }
    }

    /// 复位归零。
    pub fn reset(&mut self) {
        self.locked = false;
        self.last_flip_at = None;
        self.flips = 0;
        self.jitter_events = 0;
        self.presses = 0;
    }
}

/// 状态指纹。
pub fn caps_fp(m: &CapsMachine) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(m.locked as u8);
    match m.last_flip_at {
        Some(at) => buf.extend_from_slice(&at.to_le_bytes()),
        None => buf.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()),
    }
    buf.extend_from_slice(&m.flips.to_le_bytes());
    buf.extend_from_slice(&m.jitter_events.to_le_bytes());
    fnv1a64(&buf)
}

/// 状态行（呈现面）：locked=<0/1> flips=<n> jitter=<n>。
pub fn caps_line(m: &CapsMachine) -> String {
    let mut s = String::new();
    s.push_str("locked=");
    s.push_str(&(m.locked as u32).to_string());
    s.push_str(" flips=");
    s.push_str(&m.flips.to_string());
    s.push_str(" jitter=");
    s.push_str(&m.jitter_events.to_string());
    s
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 消除窗边界：间隔 DEBOUNCE_MS-1 不翻、恰 DEBOUNCE_MS 翻、DEBOUNCE_MS+1 翻。
    let mut m = CapsMachine::new();
    m.press(0)?; // 首按翻转 → locked
    match m.press(DEBOUNCE_MS - 1)? {
        false => {}
        true => return Err(E_BOUND),
    }
    if !m.locked {
        return Err(E_BOUND);
    }
    let mut m2 = CapsMachine::new();
    m2.press(0)?;
    match m2.press(DEBOUNCE_MS)? {
        true => {}
        false => return Err(E_BOUND),
    }
    let mut m3 = CapsMachine::new();
    m3.press(0)?;
    match m3.press(DEBOUNCE_MS + 1)? {
        true => {}
        false => return Err(E_BOUND),
    }
    hits += 1;

    // R2 快速连按状态恒定：20 笔 3ms 间隔连按（总跨 57ms < 消除窗），
    // 只在首按翻转一次。
    let mut m4 = CapsMachine::new();
    let mut t = 0u32;
    while t < 20 {
        let flipped = m4.press(t * 3)?;
        if t == 0 {
            if !flipped {
                return Err(E_SLOW);
            }
        } else if flipped {
            return Err(E_FLIP);
        }
        t += 1;
    }
    if !m4.locked || m4.flips != 1 || m4.jitter_events != 19 {
        return Err(E_FLIP);
    }
    hits += 1;

    // R3 慢速连按逐次翻转：6 笔间隔 200ms（≥ 窗），全翻，终态与奇偶一致。
    let mut m5 = CapsMachine::new();
    let mut t = 0u32;
    while t < 6 {
        let flipped = m5.press(t * 200)?;
        if !flipped {
            return Err(E_SLOW);
        }
        t += 1;
    }
    if m5.jitter_events != 0 || m5.flips != 6 {
        return Err(E_SLOW);
    }
    if !m5.state_consistent() {
        return Err(E_STATE);
    }
    // 6 次翻转 → 终态 false。
    if m5.locked {
        return Err(E_STATE);
    }
    hits += 1;

    // R4 抖动记账守恒：快慢混合流 flips+jitter == presses。
    let mut lcg = Lcg::new(0x48487);
    let mut m6 = CapsMachine::new();
    let mut at = 0u32;
    let mut n = 0u32;
    while n < 48 {
        let gap = if lcg.below(2) == 0 {
            lcg.below(DEBOUNCE_MS) // 抖动侧
        } else {
            DEBOUNCE_MS + lcg.below(200) // 翻转侧
        };
        at += gap;
        m6.press(at)?;
        n += 1;
    }
    if !m6.conserved() {
        return Err(E_CONSERV);
    }
    if !m6.state_consistent() {
        return Err(E_STATE);
    }
    hits += 1;

    // R5 混合流账目一致：快簇×3（每簇 5 笔 5ms）+ 簇间 200ms。
    // 每簇恰翻 1 次、抖 4 笔；三簇共翻 3 次、抖 12 笔。
    let mut m7 = CapsMachine::new();
    let mut cluster = 0u32;
    let mut base = 0u32;
    while cluster < 3 {
        let mut t = 0u32;
        while t < 5 {
            let at = base + t * 5;
            let flipped = m7.press(at)?;
            if t == 0 {
                if !flipped {
                    return Err(E_SLOW);
                }
            } else if flipped {
                return Err(E_FLIP);
            }
            t += 1;
        }
        base += 200 + 25;
        cluster += 1;
    }
    if m7.flips != 3 || m7.jitter_events != 12 || m7.presses != 15 {
        return Err(E_CONSERV);
    }
    if !m7.conserved() || !m7.state_consistent() {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 状态机无非法转移：任意 LCG 时戳流终态与 flips 奇偶恒一致。
    let mut m8 = CapsMachine::new();
    let mut at = 0u32;
    let mut n = 0u32;
    while n < 64 {
        at += lcg.below(400);
        m8.press(at)?;
        if !m8.state_consistent() {
            return Err(E_STATE);
        }
        n += 1;
    }
    hits += 1;

    // R7 复位归零：复位后全部字段回初值。
    m8.reset();
    if m8.locked || m8.last_flip_at.is_some() || m8.flips != 0 || m8.jitter_events != 0 || m8.presses != 0 {
        return Err(E_RESET);
    }
    hits += 1;

    // R8 指纹稳定：同机两次指纹一致且非零。
    let f1 = caps_fp(&m7);
    let f2 = caps_fp(&m7);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行齐全：locked=/flips=/jitter= 三段在位。
    let line = caps_line(&m7);
    if !has_kw(line.as_bytes(), b"locked=")
        || !has_kw(line.as_bytes(), b"flips=3")
        || !has_kw(line.as_bytes(), b"jitter=12")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
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
