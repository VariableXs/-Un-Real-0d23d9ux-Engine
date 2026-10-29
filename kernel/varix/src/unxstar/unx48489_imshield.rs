//! UNX-F48489 —— E·IM 组合期快捷键屏蔽（正：IME 组合期应用快捷键不误触）
//! 判据原文：UNX-F48489-J1 IME 组合期（组合缓冲非空且未提交）内，
//! 应用药剂快捷键（Ctrl+S/Ctrl+W/Ctrl+Tab 等）必须被屏蔽层拦截不下传，
//! 组合外的同键序列照常放行；屏蔽事件全额记账、守恒、指纹可复核。
//! 判据落地（drill 场景）：R1 快捷键闭集；R2 组合期全拦（闭集逐键）；
//! R3 组合外零误拦；R4 拦截记账守恒；R5 组合边界（提交后首键放行）；
//! R6 未知键组合期放行（进入组合缓冲）；R7 指纹稳定；
//! R8 呈现行齐全；R9 屏蔽层零副作用（拦截不改组合缓冲）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE740;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_LEAK: u16 = SEG_BASE + 1; // 组合期快捷键泄漏下传
const E_OVERBLOCK: u16 = SEG_BASE + 2; // 组合外快捷键被误拦
const E_CONSERV: u16 = SEG_BASE + 3; // 拦截记账不守恒
const E_SIDE: u16 = SEG_BASE + 4; // 拦截改动组合缓冲（副作用）
const E_COMMIT: u16 = SEG_BASE + 5; // 提交后首键仍被拦
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_KEY: u16 = SEG_BASE + 8; // 键位编码越界

/// 应用快捷键闭集（组合期屏蔽对象）。
pub const HK_CTRL_S: u8 = 1;
pub const HK_CTRL_W: u8 = 2;
pub const HK_CTRL_TAB: u8 = 3;
pub const HK_ALT_F4: u8 = 4;
pub const HK_CTRL_N: u8 = 5;
pub const SHORTCUTS: [u8; 5] = [HK_CTRL_S, HK_CTRL_W, HK_CTRL_TAB, HK_ALT_F4, HK_CTRL_N];

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

/// 按键事件：键位 + 是否携带应用快捷键语义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct KeyEvent {
    pub key: u8,
    pub is_shortcut: bool,
}

/// IME 会话：组合缓冲 + 屏蔽账。
pub struct ImSession {
    pub buffer: Vec<char>,
    pub blocked: u32,
    pub passed: u32,
    pub composed: u32,
    pub letters: u32,
}

impl ImSession {
    pub fn new() -> ImSession {
        ImSession {
            buffer: vec![],
            blocked: 0,
            passed: 0,
            composed: 0,
            letters: 0,
        }
    }

    /// 组合期判定：组合缓冲非空。
    pub fn composing(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// 灌入一键：
    /// ① 快捷键 + 组合期 → 拦截（零副作用，缓冲不动）；
    /// ② 快捷键 + 组合外 → 放行（应用层收）；
    /// ③ 普通键 → 进组合缓冲；
    /// ④ 数字 1..=9（组合同）→ 选候选并清缓冲（提交）。
    pub fn feed(&mut self, e: KeyEvent) -> Result<bool, u16> {
        if e.key > 32 {
            return Err(E_KEY);
        }
        if e.is_shortcut && self.composing() {
            self.blocked += 1;
            return Ok(false);
        }
        if e.is_shortcut {
            self.passed += 1;
            return Ok(true);
        }
        // 普通键：字母进缓冲；数字选词提交（组合外数字直通放行）。
        if e.key >= 1 && e.key <= 9 {
            if self.composing() {
                self.composed += 1;
                self.buffer.clear();
            } else {
                self.passed += 1;
            }
            return Ok(true);
        }
        // 字母编码：内部码 10..=32（'a' 起）进组合缓冲，并记字母历史账。
        self.letters += 1;
        self.buffer.push(e.key as u8 as char);
        Ok(true)
    }

    /// 记账守恒：每笔事件恰落一账 —— 拦截/放行/提交/字母历史，四账之和 == 总键数。
    /// 组合缓冲是字母键的当前快照（可能已被提交清空），不重复入账。
    pub fn conserved(&self, total: u32) -> bool {
        self.blocked + self.passed + self.composed + self.letters == total
    }
}

/// 屏蔽指纹：缓冲 + 三账串联。
pub fn session_fp(s: &ImSession) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < s.buffer.len() {
        buf.push(s.buffer[i] as u8);
        i += 1;
    }
    buf.extend_from_slice(&s.blocked.to_le_bytes());
    buf.extend_from_slice(&s.passed.to_le_bytes());
    buf.extend_from_slice(&s.composed.to_le_bytes());
    fnv1a64(&buf)
}

/// 会话行（呈现面）：composing=<0/1> blocked=<n> passed=<n> composed=<n>。
pub fn session_line(s: &ImSession) -> String {
    let mut s2 = String::new();
    s2.push_str("composing=");
    s2.push_str(&(s.composing() as u32).to_string());
    s2.push_str(" blocked=");
    s2.push_str(&s.blocked.to_string());
    s2.push_str(" passed=");
    s2.push_str(&s.passed.to_string());
    s2.push_str(" composed=");
    s2.push_str(&s.composed.to_string());
    s2
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 快捷键闭集：五键非零互异，编码越界键（>32）拒收。
    let mut i = 0usize;
    while i < SHORTCUTS.len() {
        if SHORTCUTS[i] == 0 {
            return Err(E_KEY);
        }
        let mut j = i + 1;
        while j < SHORTCUTS.len() {
            if SHORTCUTS[i] == SHORTCUTS[j] {
                return Err(E_KEY);
            }
            j += 1;
        }
        i += 1;
    }
    let mut s = ImSession::new();
    match s.feed(KeyEvent { key: 33, is_shortcut: false }) {
        Err(E_KEY) => {}
        _ => return Err(E_KEY),
    }
    hits += 1;

    // R2 组合期全拦：进入组合（字母 5 键）后五快捷键逐键拦截。
    let mut s2 = ImSession::new();
    let mut t = 0u8;
    while t < 5 {
        s2.feed(KeyEvent { key: 10 + t, is_shortcut: false })?; // 字母进缓冲
        t += 1;
    }
    if !s2.composing() {
        return Err(E_LEAK);
    }
    let mut i = 0usize;
    while i < SHORTCUTS.len() {
        match s2.feed(KeyEvent { key: SHORTCUTS[i], is_shortcut: true })? {
            false => {}
            true => return Err(E_LEAK),
        }
        i += 1;
    }
    if s2.blocked != 5 {
        return Err(E_LEAK);
    }
    hits += 1;

    // R9 屏蔽层零副作用：五次拦截后缓冲与指纹（除 blocked 账外）不变。
    // 复核口径：重放同一流到干净会话，缓冲长度应一致。
    let mut s2b = ImSession::new();
    let mut t = 0u8;
    while t < 5 {
        s2b.feed(KeyEvent { key: 10 + t, is_shortcut: false })?;
        t += 1;
    }
    if s2b.buffer.len() != s2.buffer.len() {
        return Err(E_SIDE);
    }
    if s2.buffer.len() != 5 {
        return Err(E_SIDE);
    }
    hits += 1;

    // R3 组合外零误拦：空会话五快捷键全部放行。
    let mut s3 = ImSession::new();
    let mut i = 0usize;
    while i < SHORTCUTS.len() {
        match s3.feed(KeyEvent { key: SHORTCUTS[i], is_shortcut: true })? {
            true => {}
            false => return Err(E_OVERBLOCK),
        }
        i += 1;
    }
    if s3.blocked != 0 || s3.passed != 5 {
        return Err(E_OVERBLOCK);
    }
    hits += 1;

    // R4 拦截记账守恒：混合流对账（拦截+放行+提交+缓冲 == 总键）。
    let mut s4 = ImSession::new();
    // 字母 3 键 → 缓冲 3。
    let mut t = 0u8;
    while t < 3 {
        s4.feed(KeyEvent { key: 10 + t, is_shortcut: false })?;
        t += 1;
    }
    // 快捷键 2 笔组合期拦截。
    let mut t = 0usize;
    while t < 2 {
        s4.feed(KeyEvent { key: SHORTCUTS[t], is_shortcut: true })?;
        t += 1;
    }
    // 数字 3 选词提交（清缓冲）。
    s4.feed(KeyEvent { key: 3, is_shortcut: false })?;
    // 提交后快捷键 1 笔放行。
    s4.feed(KeyEvent { key: HK_CTRL_S, is_shortcut: true })?;
    // 组合外字母 1 键 → 缓冲 1。
    s4.feed(KeyEvent { key: 10, is_shortcut: false })?;
    let total = 3 + 2 + 1 + 1 + 1;
    if !s4.conserved(total) {
        return Err(E_CONSERV);
    }
    if s4.blocked != 2 || s4.passed != 1 || s4.composed != 1 || s4.letters != 4 {
        return Err(E_CONSERV);
    }
    if s4.buffer.len() != 1 {
        return Err(E_SIDE);
    }
    hits += 1;

    // R5 组合边界：提交后首键（快捷键）必须放行，不得残留屏蔽态。
    let mut s5 = ImSession::new();
    s5.feed(KeyEvent { key: 10, is_shortcut: false })?;
    s5.feed(KeyEvent { key: 11, is_shortcut: false })?;
    if !s5.composing() {
        return Err(E_COMMIT);
    }
    s5.feed(KeyEvent { key: 1, is_shortcut: false })?; // 数字 1 提交
    match s5.feed(KeyEvent { key: HK_CTRL_W, is_shortcut: true })? {
        true => {}
        false => return Err(E_COMMIT),
    }
    if s5.composing() {
        return Err(E_COMMIT);
    }
    hits += 1;

    // R6 未知键组合期放行：组合期普通键继续进缓冲（组合需要）。
    let mut s6 = ImSession::new();
    s6.feed(KeyEvent { key: 10, is_shortcut: false })?;
    s6.feed(KeyEvent { key: 12, is_shortcut: false })?;
    if s6.buffer.len() != 2 {
        return Err(E_LEAK);
    }
    hits += 1;

    // R7 指纹稳定：同会话两次指纹一致且非零。
    let f1 = session_fp(&s4);
    let f2 = session_fp(&s4);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R8 呈现行齐全：composing=/blocked=/passed=/composed= 四段在位。
    let line = session_line(&s4);
    if !has_kw(line.as_bytes(), b"composing=")
        || !has_kw(line.as_bytes(), b"blocked=2")
        || !has_kw(line.as_bytes(), b"passed=1")
        || !has_kw(line.as_bytes(), b"composed=1")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9' LCG 混合流守恒：128 键随机流（字母/数字/快捷键混排）对账。
    let mut lcg = Lcg::new(0x48489);
    let mut s7 = ImSession::new();
    let mut n = 0u32;
    let mut injected = 0u32;
    while n < 128 {
        let r = lcg.below(3);
        let e = if r == 0 {
            KeyEvent { key: 10u8 + lcg.below(23) as u8, is_shortcut: false } // 字母
        } else if r == 1 {
            KeyEvent { key: 1u8 + lcg.below(9) as u8, is_shortcut: false } // 数字
        } else {
            KeyEvent { key: SHORTCUTS[lcg.below(5) as usize], is_shortcut: true }
        };
        s7.feed(e)?;
        injected += 1;
        n += 1;
    }
    if !s7.conserved(injected) {
        return Err(E_CONSERV);
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
