//! UNX-F48501 —— E·回注预算上限（正：keysym 回注速率有上限且常态不触顶）
//! 判据原文：UNX-F48501-J1 keysym 回注通道实行速率预算：
//! 每秒回注上限 RATE_CAP（本卡 800 枚），常态流量（≤400 枚/秒）
//! 不得触顶；超发部分按令牌桶排队拒绝并记账，不丢失账目；
//! 桶内令牌守恒、边界恰发、fuzz 流零错态、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 预算常量闭集；R2 常态流量不触顶；
//! R3 上限边界（恰发/超发一枚）；R4 超发记账守恒；
//! R5 令牌桶逐秒恢复；R6 连续秒累计不透支；
//! R7 零流量与饱和流量边界；R8 fuzz 回注流零错态；
//! R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xE940;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_CAP: u16 = SEG_BASE + 1; // 预算常量异常
const E_TOUCH: u16 = SEG_BASE + 2; // 常态流量触顶
const E_BOUND: u16 = SEG_BASE + 3; // 边界恰发/超发判定错误
const E_CONSERV: u16 = SEG_BASE + 4; // 超发记账不守恒
const E_REFILL: u16 = SEG_BASE + 5; // 令牌桶恢复异常
const E_OVERDRAFT: u16 = SEG_BASE + 6; // 跨秒累计透支
const E_EDGE: u16 = SEG_BASE + 7; // 零/饱和流量边界错
const E_FUZZ: u16 = SEG_BASE + 8; // fuzz 回注流错态
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 10; // 呈现行缺段

/// 每秒回注上限（枚）。
pub const RATE_CAP: u32 = 800;
/// 常态流量上界（枚/秒）：低于此值视为常态。
pub const NORMAL_MAX: u32 = 400;
/// 每秒桶容量（与上限一致，不跨秒结转）。
pub const BUCKET: u32 = RATE_CAP;

/// 一秒的回注请求量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecReq {
    pub sec: u32,
    pub ask: u32,
}

/// 令牌桶回注闸：逐秒发放 RATE_CAP 枚，超发拒绝并记账。
pub struct BudgetGate {
    pub tokens: u32,
    pub sec: u32,
    pub granted: u32,
    pub denied: u32,
    pub ask_total: u32,
}

impl BudgetGate {
    pub fn new() -> BudgetGate {
        BudgetGate {
            tokens: BUCKET,
            sec: 0,
            granted: 0,
            denied: 0,
            ask_total: 0,
        }
    }

    /// 请求 n 枚：返回实发数；桶空部分拒绝记账。
    pub fn request(&mut self, n: u32) -> u32 {
        let give = if n <= self.tokens { n } else { self.tokens };
        self.tokens -= give;
        self.granted += give;
        self.denied += n - give;
        self.ask_total += n;
        give
    }

    /// 进入下一秒：桶重置为 BUCKET，秒号必须单调。
    pub fn tick(&mut self, sec: u32) -> Result<(), u16> {
        if sec <= self.sec {
            return Err(E_REFILL);
        }
        self.sec = sec;
        self.tokens = BUCKET;
        Ok(())
    }

    /// 记账守恒：granted + denied == ask_total。
    pub fn conserved(&self) -> bool {
        self.granted + self.denied == self.ask_total
    }

    /// 常态不触顶：请求量 ≤ NORMAL_MAX 时拒绝数恒 0。
    pub fn normal_ok(ask: u32) -> bool {
        ask <= NORMAL_MAX
    }

    /// 秒账快照。
    pub fn snapshot(&self) -> (u32, u32, u32) {
        (self.granted, self.denied, self.tokens)
    }
}

/// 单秒结算：输入请求量，输出 (实发, 拒绝)，并校验常态语义。
pub fn settle_sec(ask: u32) -> (u32, u32) {
    let mut g = BudgetGate::new();
    let got = g.request(ask);
    (got, ask - got)
}

/// 一笔日结：连续多秒请求一次结清，输出全期实发/拒绝与透支秒数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DayReport {
    pub secs: u32,
    pub granted: u32,
    pub denied: u32,
    pub overdraft_secs: u32,
}

/// 日结器：逐秒 tick + request，透支秒 = 请求超上限的秒数。
pub fn settle_day(reqs: &[SecReq]) -> Result<DayReport, u16> {
    let mut g = BudgetGate::new();
    let mut granted = 0u32;
    let mut denied = 0u32;
    let mut overdraft = 0u32;
    let mut i = 0usize;
    while i < reqs.len() {
        let sec = reqs[i].sec;
        if i == 0 {
            if sec == 0 {
                return Err(E_REFILL);
            }
            g = BudgetGate::new();
            g.sec = sec - 1;
        }
        if sec <= g.sec {
            return Err(E_REFILL);
        }
        let gap = sec - g.sec;
        // 跨秒不结转：桶直接重置为满额。
        let mut j = 0u32;
        while j < gap {
            g.tick(g.sec + 1)?;
            j += 1;
        }
        let ask = reqs[i].ask;
        let got = g.request(ask);
        if ask > RATE_CAP {
            overdraft += 1;
        }
        granted += got;
        denied += ask - got;
        i += 1;
    }
    Ok(DayReport {
        secs: reqs.len() as u32,
        granted,
        denied,
        overdraft_secs: overdraft,
    })
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 预算常量闭集：RATE_CAP == 800、NORMAL_MAX < RATE_CAP、BUCKET == RATE_CAP。
    if RATE_CAP != 800 || NORMAL_MAX >= RATE_CAP || BUCKET != RATE_CAP {
        return Err(E_CAP);
    }
    hits += 1;

    // R2 常态流量不触顶：LCG 生成 32 秒常态请求（≤400），拒绝数恒 0。
    let mut lcg = Lcg::new(0x48501);
    let mut t = 0u32;
    while t < 32 {
        let ask = lcg.below(NORMAL_MAX + 1);
        if !BudgetGate::normal_ok(ask) {
            return Err(E_TOUCH);
        }
        let (got, denied) = settle_sec(ask);
        if got != ask || denied != 0 {
            return Err(E_TOUCH);
        }
        t += 1;
    }
    hits += 1;

    // R3 上限边界：恰发 800 全发；超发 801 恰拒 1。
    let (g1, d1) = settle_sec(RATE_CAP);
    if g1 != RATE_CAP || d1 != 0 {
        return Err(E_BOUND);
    }
    let (g2, d2) = settle_sec(RATE_CAP + 1);
    if g2 != RATE_CAP || d2 != 1 {
        return Err(E_BOUND);
    }
    hits += 1;

    // R4 超发记账守恒：大批超发（5000 枚）后 granted+denied == ask。
    let mut g = BudgetGate::new();
    let got = g.request(5000);
    if got != RATE_CAP {
        return Err(E_CONSERV);
    }
    if !g.conserved() || g.denied != 5000 - RATE_CAP {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R5 令牌桶逐秒恢复：耗尽后 tick 一秒桶满；同秒 tick 拒收。
    let mut g2 = BudgetGate::new();
    g2.request(RATE_CAP);
    if g2.tokens != 0 {
        return Err(E_REFILL);
    }
    g2.tick(1)?;
    if g2.tokens != BUCKET {
        return Err(E_REFILL);
    }
    match g2.tick(1) {
        Err(E_REFILL) => hits += 1,
        _ => return Err(E_REFILL),
    }

    // R6 连续秒累计不透支：每秒各请求 1000 枚，连续 8 秒，
    // 每秒实发恰 RATE_CAP、跨秒不透支（实发总量 == 8×RATE_CAP）。
    let mut g3 = BudgetGate::new();
    let mut sec = 1u32;
    let mut total_granted = 0u32;
    while sec <= 8 {
        g3.tick(sec)?;
        let got = g3.request(1000);
        if got != RATE_CAP {
            return Err(E_OVERDRAFT);
        }
        total_granted += got;
        sec += 1;
    }
    if total_granted != 8 * RATE_CAP || !g3.conserved() {
        return Err(E_OVERDRAFT);
    }
    hits += 1;

    // R6b 日结面：4 秒混合请求（含一笔透支），实发 = 4×RATE_CAP 内的可满足量、
    // 透支秒恰 1、全期记账守恒。
    let reqs = [
        SecReq { sec: 1, ask: 300 },
        SecReq { sec: 2, ask: RATE_CAP },
        SecReq { sec: 3, ask: RATE_CAP + 20 },
        SecReq { sec: 4, ask: 0 },
    ];
    let rep = settle_day(&reqs)?;
    if rep.secs != 4 || rep.overdraft_secs != 1 {
        return Err(E_OVERDRAFT);
    }
    let expect_granted = 300 + RATE_CAP + RATE_CAP + 0;
    if rep.granted != expect_granted || rep.denied != 20 {
        return Err(E_CONSERV);
    }
    if rep.granted + rep.denied != 300 + RATE_CAP + RATE_CAP + 20 {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 零流量与饱和流量边界：0 枚请求全发不记账；BUCKET+1000 拒发差额恰账。
    let (g0, d0) = settle_sec(0);
    if g0 != 0 || d0 != 0 {
        return Err(E_EDGE);
    }
    let (gs, ds) = settle_sec(BUCKET + 1000);
    if gs != BUCKET || ds != 1000 {
        return Err(E_EDGE);
    }
    hits += 1;

    // R8 fuzz 回注流零错态：LCG 驱动 64 秒混合流量（0..=2000），
    // 逐秒守恒、逐秒实发 ≤ RATE_CAP、常态秒零拒绝。
    let mut g4 = BudgetGate::new();
    let mut lcg2 = Lcg::new(0x25F1_10);
    let mut sec = 1u32;
    let mut t = 0u32;
    while t < 64 {
        g4.tick(sec)?;
        let ask = lcg2.below(2001);
        let got = g4.request(ask);
        if got > RATE_CAP {
            return Err(E_FUZZ);
        }
        if ask <= NORMAL_MAX && got != ask {
            return Err(E_TOUCH);
        }
        if g4.granted + g4.denied != g4.ask_total {
            return Err(E_FUZZ);
        }
        sec += 1;
        t += 1;
    }
    if !g4.conserved() {
        return Err(E_FUZZ);
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = gate_fp(&g4);
    let f2 = gate_fp(&g4);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：cap=/granted=/denied= 三段前缀在位。
    let line = gate_line(&g4);
    if !has_kw(line.as_bytes(), b"cap=")
        || !has_kw(line.as_bytes(), b"granted=")
        || !has_kw(line.as_bytes(), b"denied=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
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

/// 账面指纹：秒号 + 四项记账串联。
pub fn gate_fp(g: &BudgetGate) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&g.sec.to_le_bytes());
    buf.extend_from_slice(&g.granted.to_le_bytes());
    buf.extend_from_slice(&g.denied.to_le_bytes());
    buf.extend_from_slice(&g.ask_total.to_le_bytes());
    buf.extend_from_slice(&g.tokens.to_le_bytes());
    fnv1a64(&buf)
}

/// 呈现行：cap=<上限> granted=<实发> denied=<拒绝>。
pub fn gate_line(g: &BudgetGate) -> String {
    let mut s = String::new();
    s.push_str("cap=");
    s.push_str(&RATE_CAP.to_string());
    s.push_str(" granted=");
    s.push_str(&g.granted.to_string());
    s.push_str(" denied=");
    s.push_str(&g.denied.to_string());
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
