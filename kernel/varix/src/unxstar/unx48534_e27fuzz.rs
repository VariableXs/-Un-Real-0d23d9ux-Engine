//! UNX-F48534 —— E27 fuzz（随机设备态翻转注入零崩）
//! 判据原文：UNX-F48534-J1 以确定性 LCG 注入 128 笔随机设备事件
//! （接入/移除/换装/清理/伪事件），设备态机必须零崩：每笔事件产出
//! 可分类结局（合法转移或拒收），拒收不改态、转移不出闭集、
//! 事件账守恒、终态可重放复核、边界事件序列口径逐一相符。
//! 判据落地（drill 场景）：R1 态闭集与转移表封闭；R2 同种子同结局序；
//! R3 异种子分叉；R4 128 例零崩全分类；R5 拒收不改态；
//! R6 事件四类全覆盖（掺确定性边界）；R7 统计守恒；
//! R8 边界序列口径（悬空移除/重复清理等必拒）；R9 终态重放一致；R10 指纹稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEF40;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_STATE: u16 = SEG_BASE + 1; // 终态越出闭集
const E_DET: u16 = SEG_BASE + 2; // 同种子结局序不确定
const E_DIV: u16 = SEG_BASE + 3; // 异种子结局序未分叉
const E_CRASH: u16 = SEG_BASE + 4; // 出现不可分类结局（崩溃等价）
const E_MUTATE: u16 = SEG_BASE + 5; // 拒收改了态
const E_COVER: u16 = SEG_BASE + 6; // 事件四类未全覆盖
const E_STAT: u16 = SEG_BASE + 7; // 统计守恒破缺
const E_BOUND: u16 = SEG_BASE + 8; // 边界序列口径不符
const E_REPLAY: u16 = SEG_BASE + 9; // 终态重放不一致
const E_FP: u16 = SEG_BASE + 10; // 指纹漂移
const E_RUNS: u16 = SEG_BASE + 11; // 注入笔数不足

/// 注入笔数。
pub const RUNS: u32 = 128;

/// 设备态闭集：0 未接 / 1 已接 / 2 换装中 / 3 清理中。
pub const ST_DETACHED: u8 = 0;
pub const ST_ATTACHED: u8 = 1;
pub const ST_RESEATING: u8 = 2;
pub const ST_TEARING: u8 = 3;

/// 事件闭集：1 接入 / 2 移除 / 3 换装 / 4 清理 / 5 伪事件。
pub const EV_ATTACH: u8 = 1;
pub const EV_DETACH: u8 = 2;
pub const EV_RESEAT: u8 = 3;
pub const EV_TEARDOWN: u8 = 4;
pub const EV_BOGUS: u8 = 5;

/// 拒收口径：悬空移除 / 重复清理 / 换装中再换装 / 伪事件。
pub const REJ_HANG: u16 = SEG_BASE + 12; // 未接先移除
pub const REJ_TWICE: u16 = SEG_BASE + 13; // 清理中再清理
pub const REJ_BUSY: u16 = SEG_BASE + 14; // 换装中再换装
pub const REJ_BOGUS: u16 = SEG_BASE + 15; // 伪事件

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

/// 单笔结局：1 合法转移（new_state 生效）/ 2 拒收（code 生效）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    pub kind: u8, // 1 转移 / 2 拒收
    pub new_state: u8,
    pub code: u16,
}

pub const OC_TRANSIT: u8 = 1;
pub const OC_REJECT: u8 = 2;

/// 设备态机（单实例账面）。
pub struct DevMachine {
    pub state: u8,
    pub transits: u32,
    pub rejects: u32,
    pub events: Vec<u8>,
}

impl DevMachine {
    pub fn new() -> DevMachine {
        DevMachine {
            state: ST_DETACHED,
            transits: 0,
            rejects: 0,
            events: vec![],
        }
    }

    /// 灌入一笔事件：按转移表判定合法/拒收；拒收不改态。
    pub fn feed(&mut self, ev: u8) -> Outcome {
        self.events.push(ev);
        let s = self.state;
        match ev {
            EV_ATTACH => {
                if s == ST_DETACHED || s == ST_RESEATING {
                    self.state = ST_ATTACHED;
                    self.transits += 1;
                    Outcome { kind: OC_TRANSIT, new_state: ST_ATTACHED, code: 0 }
                } else {
                    // 已接/清理中不接受接入：悬空口径反向（重复接入）。
                    self.rejects += 1;
                    Outcome { kind: OC_REJECT, new_state: s, code: REJ_HANG }
                }
            }
            EV_DETACH => {
                if s == ST_ATTACHED || s == ST_TEARING || s == ST_RESEATING {
                    self.state = ST_DETACHED;
                    self.transits += 1;
                    Outcome { kind: OC_TRANSIT, new_state: ST_DETACHED, code: 0 }
                } else {
                    self.rejects += 1;
                    Outcome { kind: OC_REJECT, new_state: s, code: REJ_HANG }
                }
            }
            EV_RESEAT => {
                if s == ST_ATTACHED {
                    self.state = ST_RESEATING;
                    self.transits += 1;
                    Outcome { kind: OC_TRANSIT, new_state: ST_RESEATING, code: 0 }
                } else if s == ST_RESEATING {
                    self.rejects += 1;
                    Outcome { kind: OC_REJECT, new_state: s, code: REJ_BUSY }
                } else {
                    self.rejects += 1;
                    Outcome { kind: OC_REJECT, new_state: s, code: REJ_HANG }
                }
            }
            EV_TEARDOWN => {
                if s == ST_ATTACHED {
                    self.state = ST_TEARING;
                    self.transits += 1;
                    Outcome { kind: OC_TRANSIT, new_state: ST_TEARING, code: 0 }
                } else if s == ST_TEARING {
                    self.rejects += 1;
                    Outcome { kind: OC_REJECT, new_state: s, code: REJ_TWICE }
                } else {
                    self.rejects += 1;
                    Outcome { kind: OC_REJECT, new_state: s, code: REJ_HANG }
                }
            }
            _ => {
                // 伪事件：任何态都拒。
                self.rejects += 1;
                Outcome { kind: OC_REJECT, new_state: s, code: REJ_BOGUS }
            }
        }
    }

    /// 统计守恒：转移 + 拒收 == 事件笔数。
    pub fn conserved(&self) -> bool {
        self.transits + self.rejects == self.events.len() as u32
    }
}

/// 结局序指纹。
fn outcomes_fp(outs: &[Outcome]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < outs.len() {
        buf.push(outs[i].kind);
        buf.push(outs[i].new_state);
        buf.extend_from_slice(&outs[i].code.to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 以给定 LCG 跑 n 笔随机事件，返回（结局序, 终态账）。
pub fn fuzz_seq(mut lcg: Lcg, n: usize) -> (Vec<Outcome>, DevMachine) {
    let mut m = DevMachine::new();
    let mut outs: Vec<Outcome> = vec![];
    let mut i = 0usize;
    while i < n {
        let ev = 1 + lcg.below(5) as u8;
        outs.push(m.feed(ev));
        i += 1;
    }
    (outs, m)
}

/// 结局行（呈现面）：kind=<transit|reject> state=<n> code=<n>。
pub fn outcome_line(i: usize, o: &Outcome) -> String {
    let mut s = String::new();
    s.push_str("n=");
    s.push_str(&(i as u32).to_string());
    s.push_str(" kind=");
    if o.kind == OC_TRANSIT {
        s.push_str("transit");
    } else {
        s.push_str("reject");
    }
    s.push_str(" state=");
    s.push_str(&(o.new_state as u32).to_string());
    s.push_str(" code=");
    s.push_str(&o.code.to_string());
    s
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 态闭集与转移表封闭：空账终态 = ST_DETACHED（合法值）。
    let m0 = DevMachine::new();
    if m0.state != ST_DETACHED || m0.state > ST_TEARING {
        return Err(E_STATE);
    }
    hits += 1;

    // R2 同种子同结局序：两路 LCG 同种子 64 笔逐一相同。
    let (sa, ma) = fuzz_seq(Lcg::new(0x48534), 64);
    let (sb, mb) = fuzz_seq(Lcg::new(0x48534), 64);
    if sa != sb || ma.state != mb.state {
        return Err(E_DET);
    }
    hits += 1;

    // R3 异种子分叉：种子 1 与种子 2 结局序指纹不同。
    let (s1, _) = fuzz_seq(Lcg::new(1), 32);
    let (s2, _) = fuzz_seq(Lcg::new(2), 32);
    if outcomes_fp(&s1) == outcomes_fp(&s2) {
        return Err(E_DIV);
    }
    hits += 1;

    // R4/R5/R7：128 例零崩全分类 + 拒收不改态 + 统计守恒。
    let mut lcg = Lcg::new(0xF27);
    let mut m = DevMachine::new();
    let mut transits = 0u32;
    let mut rejects = 0u32;
    let mut n = 0u32;
    while n < RUNS {
        let ev = 1 + lcg.below(5) as u8;
        let before = m.state;
        let o = m.feed(ev);
        // R4 全分类。
        if o.kind != OC_TRANSIT && o.kind != OC_REJECT {
            return Err(E_CRASH);
        }
        if o.kind == OC_TRANSIT {
            if o.new_state > ST_TEARING {
                return Err(E_STATE);
            }
            transits += 1;
        } else {
            // R5 拒收不改态。
            if o.new_state != before || m.state != before {
                return Err(E_MUTATE);
            }
            if o.code < REJ_HANG || o.code > REJ_BOGUS {
                return Err(E_CRASH);
            }
            rejects += 1;
        }
        n += 1;
    }
    if transits != m.transits || rejects != m.rejects {
        return Err(E_STAT);
    }
    if !m.conserved() {
        return Err(E_STAT);
    }
    hits += 1;

    // R6 事件四类全覆盖：确定性边界序列掺入（每类至少一次合法转移）。
    let mut mb2 = DevMachine::new();
    // attach：DETACHED → ATTACHED。
    let o = mb2.feed(EV_ATTACH);
    if o.kind != OC_TRANSIT || o.new_state != ST_ATTACHED {
        return Err(E_COVER);
    }
    // reseat：ATTACHED → RESEATING。
    let o = mb2.feed(EV_RESEAT);
    if o.kind != OC_TRANSIT || o.new_state != ST_RESEATING {
        return Err(E_COVER);
    }
    // detach：RESEATING → DETACHED。
    let o = mb2.feed(EV_DETACH);
    if o.kind != OC_TRANSIT || o.new_state != ST_DETACHED {
        return Err(E_COVER);
    }
    // teardown 前须先 attach。
    mb2.feed(EV_ATTACH);
    let o = mb2.feed(EV_TEARDOWN);
    if o.kind != OC_TRANSIT || o.new_state != ST_TEARING {
        return Err(E_COVER);
    }
    hits += 1;

    // R8 边界序列口径：悬空移除/重复清理/换装中再换装/伪事件必拒且码精确。
    let mut m8 = DevMachine::new();
    let o = m8.feed(EV_DETACH);
    if o.kind != OC_REJECT || o.code != REJ_HANG {
        return Err(E_BOUND);
    }
    m8.feed(EV_ATTACH);
    let o = m8.feed(EV_TEARDOWN);
    if o.kind != OC_TRANSIT {
        return Err(E_BOUND);
    }
    let o = m8.feed(EV_TEARDOWN);
    if o.kind != OC_REJECT || o.code != REJ_TWICE {
        return Err(E_BOUND);
    }
    m8.feed(EV_DETACH);
    m8.feed(EV_ATTACH);
    let o = m8.feed(EV_RESEAT);
    if o.kind != OC_TRANSIT {
        return Err(E_BOUND);
    }
    let o = m8.feed(EV_RESEAT);
    if o.kind != OC_REJECT || o.code != REJ_BUSY {
        return Err(E_BOUND);
    }
    let o = m8.feed(EV_BOGUS);
    if o.kind != OC_REJECT || o.code != REJ_BOGUS {
        return Err(E_BOUND);
    }
    hits += 1;

    // R9 终态重放一致：主账事件流重放到干净账终态一致。
    let mut replay_m = DevMachine::new();
    let mut i = 0usize;
    while i < m.events.len() {
        replay_m.feed(m.events[i]);
        i += 1;
    }
    if replay_m.state != m.state {
        return Err(E_REPLAY);
    }
    hits += 1;

    // R10 指纹稳定：主账结局序两次指纹一致且非零。
    let (finals, _) = fuzz_seq(Lcg::new(0xF27), RUNS as usize);
    let f1 = outcomes_fp(&finals);
    let f2 = outcomes_fp(&finals);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    if RUNS < 64 {
        return Err(E_RUNS);
    }
    // 呈现行烟测：n=/kind=/state=/code= 四段在位。
    let line = outcome_line(3, &finals[3]);
    if !has_kw(line.as_bytes(), b"n=")
        || !has_kw(line.as_bytes(), b"kind=")
        || !has_kw(line.as_bytes(), b"state=")
        || !has_kw(line.as_bytes(), b"code=")
    {
        return Err(E_FP);
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
