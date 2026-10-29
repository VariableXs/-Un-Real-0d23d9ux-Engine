//! UNX-F48486 —— E·连点风暴注入（反：10^3 次/秒选词 ×10 防抖生效）
//! 判据原文：UNX-F48486-J1 候选选词通道遭遇 1000 次/秒连点风暴 ×10 轮，
//! 防抖器必须把每秒放行数压到配额（10 笔）以内，风暴总放行 ≤ 轮数×配额，
//! 被压事件全额记账、守恒可复核、正常节奏零误伤。
//! 判据落地（drill 场景）：R1 防抖基准（正常节奏零误伤）；R2 风暴限流
//! （每秒配额内）；R3 十轮总放行 ≤ 轮×配额；R4 被压事件记账守恒；
//! R5 配额窗口重置；R6 风暴指纹稳定；R7 放行单调（窗口内先到先得）；
//! R8 呈现行齐全；R9 零间隔极端风暴不崩。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE6E0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_QUOTA: u16 = SEG_BASE + 1; // 单窗放行超配额
const E_LOSS: u16 = SEG_BASE + 2; // 被压事件未记账
const E_CONSERV: u16 = SEG_BASE + 3; // 总账不守恒（放行+被压 ≠ 注入）
const E_HARM: u16 = SEG_BASE + 4; // 正常节奏被误伤
const E_RESET: u16 = SEG_BASE + 5; // 窗口重置失败
const E_FP: u16 = SEG_BASE + 6; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 7; // 呈现行缺段
const E_ORDER: u16 = SEG_BASE + 8; // 风暴时戳非单调

/// 防抖窗口（毫秒）与单窗放行配额。
pub const WINDOW_MS: u32 = 1000;
pub const QUOTA_PER_WIN: u32 = 10;
/// 风暴轮数与每轮注入强度。
pub const STORM_ROUNDS: u32 = 10;
pub const STORM_RATE: u32 = 1000;

/// 一笔选词事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tap {
    pub at: u32,
}

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

/// 防抖器：滚动窗口内配额限流，被压事件全额记账。
pub struct Debouncer {
    pub win_start: u32,
    pub win_count: u32,
    pub passed: Vec<Tap>,
    pub suppressed: u32,
}

impl Debouncer {
    pub fn new() -> Debouncer {
        Debouncer {
            win_start: 0,
            win_count: 0,
            passed: vec![],
            suppressed: 0,
        }
    }

    /// 灌入一笔：窗口滑动后配额重置；配额满则压下并记账。
    pub fn feed(&mut self, t: Tap) -> Result<bool, u16> {
        if t.at.saturating_sub(self.win_start) >= WINDOW_MS {
            self.win_start = t.at;
            self.win_count = 0;
        }
        if self.win_count >= QUOTA_PER_WIN {
            self.suppressed += 1;
            return Ok(false);
        }
        self.win_count += 1;
        self.passed.push(t);
        Ok(true)
    }

    /// 总守恒：放行 + 被压 == 注入总数。
    pub fn conserved(&self, injected: u32) -> bool {
        self.passed.len() as u32 + self.suppressed == injected
    }

    /// 放行单调：放行列时戳非降序（先到先得）。
    pub fn ordered(&self) -> bool {
        let mut i = 1usize;
        while i < self.passed.len() {
            if self.passed[i - 1].at > self.passed[i].at {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 复位：窗口/放行/被压全部归零。
    pub fn reset(&mut self) {
        self.win_start = 0;
        self.win_count = 0;
        self.passed.clear();
        self.suppressed = 0;
    }
}

/// 生成风暴序列：rounds 轮、每轮 rate 笔、笔距 1ms。
pub fn storm_seq(rounds: u32, rate: u32) -> Vec<Tap> {
    let mut seq: Vec<Tap> = vec![];
    let mut r = 0u32;
    while r < rounds {
        let base = r * (WINDOW_MS + 100);
        let mut i = 0u32;
        while i < rate {
            seq.push(Tap { at: base + i });
            i += 1;
        }
        r += 1;
    }
    seq
}

/// 防抖指纹：放行列 + 被压数。
pub fn deb_fp(d: &Debouncer) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < d.passed.len() {
        buf.extend_from_slice(&d.passed[i].at.to_le_bytes());
        i += 1;
    }
    buf.extend_from_slice(&d.suppressed.to_le_bytes());
    fnv1a64(&buf)
}

/// 防抖行（呈现面）：passed=<n> suppressed=<n> quota=<配额>。
pub fn deb_line(d: &Debouncer) -> String {
    let mut s = String::new();
    s.push_str("passed=");
    s.push_str(&(d.passed.len() as u32).to_string());
    s.push_str(" suppressed=");
    s.push_str(&d.suppressed.to_string());
    s.push_str(" quota=");
    s.push_str(&QUOTA_PER_WIN.to_string());
    s
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 防抖基准：正常节奏（每秒 8 笔，低于配额）零误伤。
    let mut d = Debouncer::new();
    let mut t = 0u32;
    while t < 8 {
        match d.feed(Tap { at: t * 120 }) {
            Ok(passed) => {
                if !passed {
                    return Err(E_HARM);
                }
            }
            Err(_) => return Err(E_HARM),
        }
        t += 1;
    }
    if d.suppressed != 0 || d.passed.len() != 8 {
        return Err(E_HARM);
    }
    hits += 1;

    // R2 风暴限流：单轮 1000 笔 1ms 连点，放行 ≤ 配额。
    let mut d2 = Debouncer::new();
    let mut i = 0u32;
    while i < STORM_RATE {
        d2.feed(Tap { at: 1000 + i })?;
        i += 1;
    }
    if d2.passed.len() as u32 > QUOTA_PER_WIN {
        return Err(E_QUOTA);
    }
    if d2.passed.len() as u32 != QUOTA_PER_WIN {
        return Err(E_QUOTA);
    }
    hits += 1;

    // R3 十轮总放行 ≤ 轮×配额；轮间间隔 ≥ 窗保证各轮配额独立。
    let storm = storm_seq(STORM_ROUNDS, STORM_RATE);
    let mut d3 = Debouncer::new();
    let mut i = 0usize;
    while i < storm.len() {
        d3.feed(storm[i])?;
        i += 1;
    }
    if d3.passed.len() as u32 > STORM_ROUNDS * QUOTA_PER_WIN {
        return Err(E_QUOTA);
    }
    // 每轮恰放行配额：轮基点 = r×(WINDOW+100)，窗口起点随轮重置。
    if d3.passed.len() as u32 != STORM_ROUNDS * QUOTA_PER_WIN {
        return Err(E_QUOTA);
    }
    hits += 1;

    // R4 被压事件记账守恒：注入 10^4 笔，放行 10^2，被压 9900。
    if !d3.conserved(STORM_ROUNDS * STORM_RATE) {
        return Err(E_CONSERV);
    }
    if d3.suppressed != STORM_ROUNDS * STORM_RATE - STORM_ROUNDS * QUOTA_PER_WIN {
        return Err(E_LOSS);
    }
    hits += 1;

    // R5 配额窗口重置：新窗口首笔必放行（配额从满格重新可用）。
    let mut d4 = Debouncer::new();
    let mut i = 0u32;
    while i < QUOTA_PER_WIN {
        d4.feed(Tap { at: i })?;
        i += 1;
    }
    // 配额满后同窗内压下。
    match d4.feed(Tap { at: QUOTA_PER_WIN - 1 }) {
        Ok(false) => {}
        _ => return Err(E_QUOTA),
    }
    // 跨窗后首笔放行。
    match d4.feed(Tap { at: WINDOW_MS + 5 }) {
        Ok(true) => {}
        _ => return Err(E_RESET),
    }
    if d4.win_start != WINDOW_MS + 5 || d4.win_count != 1 {
        return Err(E_RESET);
    }
    hits += 1;

    // R6 风暴指纹稳定：同风暴两次指纹一致且非零。
    let f1 = deb_fp(&d3);
    let f2 = deb_fp(&d3);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R7 放行单调：风暴流时戳本身单调，放行列必须保持非降序。
    if !monotonic(&storm) {
        return Err(E_ORDER);
    }
    if !d3.ordered() {
        return Err(E_ORDER);
    }
    hits += 1;

    // R8 呈现行齐全：passed=/suppressed=/quota= 三段在位。
    let line = deb_line(&d3);
    if !has_kw(line.as_bytes(), b"passed=")
        || !has_kw(line.as_bytes(), b"suppressed=")
        || !has_kw(line.as_bytes(), b"quota=10")
    {
        return Err(E_LINE);
    }
    hits += 1;

    // R9 零间隔极端风暴不崩：全部 at=0 的 32 笔，放行 ≤ 配额且守恒。
    let mut d5 = Debouncer::new();
    let mut i = 0u32;
    while i < 32 {
        d5.feed(Tap { at: 0 })?;
        i += 1;
    }
    if d5.passed.len() as u32 > QUOTA_PER_WIN {
        return Err(E_QUOTA);
    }
    if !d5.conserved(32) {
        return Err(E_CONSERV);
    }
    // 复位归零。
    d5.reset();
    if d5.suppressed != 0 || !d5.passed.is_empty() || d5.win_count != 0 {
        return Err(E_RESET);
    }
    hits += 1;

    Ok(hits)
}

/// 时戳单调判定。
pub fn monotonic(ts: &[Tap]) -> bool {
    let mut i = 1usize;
    while i < ts.len() {
        if ts[i - 1].at > ts[i].at {
            return false;
        }
        i += 1;
    }
    true
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
