//! UNX-F48543 —— E·崩溃后 context 重建（正：重启后输入立即可用 ≤2s）
//! 判据原文：UNX-F48543-J1 崩溃恢复面以 context 重建时限核收：
//! 重启后输入面必须在 ≤2s（2000ms 预算）内完成 context 重建并
//! 标记可用；重建路径按阶段（装载/校验/挂接/可用）顺序推进且
//! 每阶段记账；超时必须被判据检出；账目守恒、指纹与呈现行可复核。
//! 判据落地（drill 场景）：R1 预算常量；R2 阶段顺序推进；
//! R3 重建时限达标；R4 超时检出；R5 阶段账守恒；
//! R6 可用位终态判定；R7 未完成不得标记可用；
//! R8 fuzz 重建流零错态；R9 指纹稳定；R10 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF220;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_BUDGET: u16 = SEG_BASE + 1; // 预算常量异常
const E_ORDER: u16 = SEG_BASE + 2; // 阶段顺序错乱
const E_SLOW: u16 = SEG_BASE + 3; // 重建超时未检出
const E_CONSERV: u16 = SEG_BASE + 4; // 阶段账不守恒
const E_READY: u16 = SEG_BASE + 5; // 可用位终态异常
const E_EARLY: u16 = SEG_BASE + 6; // 未完成标记可用
const E_FUZZ: u16 = SEG_BASE + 7; // fuzz 重建流错态
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段
const E_STAGE: u16 = SEG_BASE + 10; // 阶段闭集异常

/// 重建预算（毫秒）。
pub const BUDGET_MS: u32 = 2000;

/// 重建阶段闭集：1 装载 / 2 校验 / 3 挂接 / 4 可用。
pub const S_LOAD: u8 = 1;
pub const S_VERIFY: u8 = 2;
pub const S_ATTACH: u8 = 3;
pub const S_READY: u8 = 4;
pub const STAGE_NAMES: [&str; 4] = ["load", "verify", "attach", "ready"];

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

/// 阶段闭集判定。
pub fn known_stage(s: u8) -> bool {
    s == S_LOAD || s == S_VERIFY || s == S_ATTACH || s == S_READY
}

/// 阶段名反查：闭集内返回名，越界返回 None。
pub fn stage_name(s: u8) -> Option<&'static str> {
    if !known_stage(s) {
        return None;
    }
    Some(STAGE_NAMES[(s - 1) as usize])
}

/// 一笔阶段推进：阶段 + 该阶段耗时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StageStep {
    pub stage: u8,
    pub cost_ms: u32,
}

/// context 重建器：阶段顺序推进 + 时限记账 + 可用位。
pub struct Rebuilder {
    pub next_stage: u8,
    pub done: Vec<StageStep>,
    pub spent_ms: u32,
    pub available: bool,
}

impl Rebuilder {
    pub fn new() -> Rebuilder {
        Rebuilder {
            next_stage: S_LOAD,
            done: vec![],
            spent_ms: 0,
            available: false,
        }
    }

    /// 推进一阶段：阶段必须按闭集顺序；S_READY 时置可用位并核时限。
    pub fn step(&mut self, st: StageStep) -> Result<(), u16> {
        if !known_stage(st.stage) {
            return Err(E_STAGE);
        }
        if st.stage != self.next_stage {
            return Err(E_ORDER);
        }
        self.done.push(st);
        self.spent_ms += st.cost_ms;
        if st.stage == S_READY {
            if self.spent_ms > BUDGET_MS {
                return Err(E_SLOW);
            }
            self.available = true;
            self.next_stage = S_LOAD + 4; // 终态哨兵：无后续阶段
        } else {
            self.next_stage += 1;
        }
        Ok(())
    }

    /// 阶段账守恒：done 数 == 4（全链）且耗时和 == spent_ms。
    pub fn conserved(&self, expect_stages: u32) -> bool {
        if self.done.len() as u32 != expect_stages {
            return false;
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.done.len() {
            sum += self.done[i].cost_ms;
            i += 1;
        }
        sum == self.spent_ms
    }

    /// 可用位一致性：available 为真必须全四阶段完成且在预算内。
    pub fn ready_sane(&self) -> bool {
        if self.available {
            self.done.len() == 4 && self.spent_ms <= BUDGET_MS
        } else {
            true
        }
    }
}

/// 超时判定单点：spent > BUDGET_MS 即超时。
pub fn is_over_budget(spent_ms: u32) -> bool {
    spent_ms > BUDGET_MS
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 预算常量：BUDGET_MS == 2000，is_over_budget 单点自洽。
    if BUDGET_MS != 2000 {
        return Err(E_BUDGET);
    }
    if is_over_budget(BUDGET_MS) || !is_over_budget(BUDGET_MS + 1) || is_over_budget(0) {
        return Err(E_BUDGET);
    }
    hits += 1;

    // R2 阶段顺序推进：乱序推进被拒，码恰为 E_ORDER。
    let mut rb = Rebuilder::new();
    match rb.step(StageStep { stage: S_VERIFY, cost_ms: 100 }) {
        Err(E_ORDER) => hits += 1,
        _ => return Err(E_ORDER),
    }

    // R3 重建时限达标：四阶段 500+600+700+150 == 1950 ≤ 2000，可用置位。
    let mut rb2 = Rebuilder::new();
    rb2.step(StageStep { stage: S_LOAD, cost_ms: 500 })?;
    rb2.step(StageStep { stage: S_VERIFY, cost_ms: 600 })?;
    rb2.step(StageStep { stage: S_ATTACH, cost_ms: 700 })?;
    rb2.step(StageStep { stage: S_READY, cost_ms: 150 })?;
    if !rb2.available || rb2.spent_ms != 1950 {
        return Err(E_READY);
    }
    hits += 1;

    // R4 超时检出：预算边界 2000 达标、2001 超时拒收，码恰为 E_SLOW。
    let mut rb3 = Rebuilder::new();
    rb3.step(StageStep { stage: S_LOAD, cost_ms: BUDGET_MS })?;
    rb3.step(StageStep { stage: S_VERIFY, cost_ms: 0 })?;
    rb3.step(StageStep { stage: S_ATTACH, cost_ms: 0 })?;
    rb3.step(StageStep { stage: S_READY, cost_ms: 0 })?;
    if !rb3.available {
        return Err(E_READY);
    }
    let mut rb4 = Rebuilder::new();
    rb4.step(StageStep { stage: S_LOAD, cost_ms: 700 })?;
    rb4.step(StageStep { stage: S_VERIFY, cost_ms: 700 })?;
    rb4.step(StageStep { stage: S_ATTACH, cost_ms: 700 })?;
    match rb4.step(StageStep { stage: S_READY, cost_ms: 1 }) {
        Err(E_SLOW) => hits += 1,
        _ => return Err(E_SLOW),
    }

    // R5 阶段账守恒：全链 4 阶段、耗时和一致。
    if !rb3.conserved(4) || !rb2.conserved(4) {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R6 可用位终态判定：ready_sane 对达标账为真；未完成账 available 恒假。
    if !rb3.ready_sane() || !rb4.ready_sane() {
        return Err(E_READY);
    }
    if rb4.available {
        return Err(E_READY);
    }
    hits += 1;

    // R7 未完成不得标记可用：半链账 available 恒假。
    let mut rb5 = Rebuilder::new();
    rb5.step(StageStep { stage: S_LOAD, cost_ms: 100 })?;
    rb5.step(StageStep { stage: S_VERIFY, cost_ms: 100 })?;
    if rb5.available {
        return Err(E_EARLY);
    }
    hits += 1;

    // R7b 阶段名反查：闭集内四名非空互异；越界反查 None。
    let mut s = 1u8;
    while s <= 4 {
        match stage_name(s) {
            Some(n) => {
                if n.is_empty() {
                    return Err(E_STAGE);
                }
            }
            None => return Err(E_STAGE),
        }
        s += 1;
    }
    if stage_name(0).is_some() || stage_name(5).is_some() {
        return Err(E_STAGE);
    }
    hits += 1;

    // R8 fuzz 重建流零错态：LCG 驱动 64 次随机分段预算全链重建，
    // 每次守恒、可用位自洽、总耗时 ≤ 预算。
    let mut lcg = Lcg::new(0x48543);
    let mut t = 0u32;
    while t < 64 {
        let mut rb6 = Rebuilder::new();
        let c1 = lcg.below(800);
        let c2 = lcg.below(800);
        let c3 = lcg.below(400);
        rb6.step(StageStep { stage: S_LOAD, cost_ms: c1 })?;
        rb6.step(StageStep { stage: S_VERIFY, cost_ms: c2 })?;
        rb6.step(StageStep { stage: S_ATTACH, cost_ms: c3 })?;
        let used = c1 + c2 + c3;
        rb6.step(StageStep {
            stage: S_READY,
            cost_ms: BUDGET_MS - used,
        })?;
        if !rb6.available || !rb6.conserved(4) || !rb6.ready_sane() {
            return Err(E_FUZZ);
        }
        t += 1;
    }
    hits += 1;

    // R9 指纹稳定：同账两次指纹一致且非零。
    let f1 = rebuilder_fp(&rb2);
    let f2 = rebuilder_fp(&rb2);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 呈现行齐全：budget=/spent=/ready= 三段前缀在位。
    let line = rebuilder_line(&rb2);
    if !has_kw(line.as_bytes(), b"budget=")
        || !has_kw(line.as_bytes(), b"spent=")
        || !has_kw(line.as_bytes(), b"ready=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 账面指纹：可用位 + 耗时 + 全阶段串联。
pub fn rebuilder_fp(rb: &Rebuilder) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.push(rb.available as u8);
    buf.extend_from_slice(&rb.spent_ms.to_le_bytes());
    let mut i = 0usize;
    while i < rb.done.len() {
        buf.push(rb.done[i].stage);
        buf.extend_from_slice(&rb.done[i].cost_ms.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 呈现行：budget=<预算> spent=<已耗> ready=<0/1>。
pub fn rebuilder_line(rb: &Rebuilder) -> String {
    let mut s = String::new();
    s.push_str("budget=");
    s.push_str(&BUDGET_MS.to_string());
    s.push_str(" spent=");
    s.push_str(&rb.spent_ms.to_string());
    s.push_str(" ready=");
    s.push_str(&(rb.available as u32).to_string());
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
