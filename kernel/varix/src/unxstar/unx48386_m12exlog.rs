//! UNX-F48386 —— M12 体验日志段汇总（B19 挫败信号段报表）
//! 判据原文（UNX-F48386-J1）：
//!   ① 纯功能代码 ≥297 行；
//!   ② B19 挫败信号五类闭集（重试风暴/暴怒点击/死点/超时/回退环路）全量入段；
//!   ③ 信号结构校验：强度权值越界拒收、会话号非零、时间戳单调；
//!   ④ 段报表：五类计数与权值并账对平；Top-N 重演与全量序一致；
//!   ⑤ 阈值闸：单类权值占比超段阈值即降级标注，降级路径确定性可复跑；
//!   ⑥ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xB1E0; // 卡面分配段

const E_TYPE_UNDEF: u16 = SEG_BASE + 1;  // 信号类型越五类闭集
const E_INTENSITY: u16 = SEG_BASE + 2;   // 强度权值越界（1..=9 之外）
const E_SESSION_ZERO: u16 = SEG_BASE + 3; // 会话号为零
const E_TS_ORDER: u16 = SEG_BASE + 4;    // 时间戳非单调
const E_DUP_EVT: u16 = SEG_BASE + 5;     // 同会话同类型同时间戳重复事件
const E_BAL: u16 = SEG_BASE + 6;         // 报表并账与全量对平失败
const E_TOPN: u16 = SEG_BASE + 7;        // Top-N 重演与全量序不一致
const E_DEGRADE: u16 = SEG_BASE + 8;     // 降级标注非确定
const E_RERUN: u16 = SEG_BASE + 9;       // 复跑指纹不一致
const E_RATIO: u16 = SEG_BASE + 10;      // 占比合计越 100%窗
const E_REPORT_EMPTY: u16 = SEG_BASE + 11; // 段报表为空

pub const SIG_RETRY_STORM: u8 = 0;
pub const SIG_RAGE_CLICK: u8 = 1;
pub const SIG_DEAD_CLICK: u8 = 2;
pub const SIG_TIMEOUT: u8 = 3;
pub const SIG_BACKNAV_LOOP: u8 = 4;
pub const SIG_KINDS: usize = 5;

pub const INTENSITY_MAX: u8 = 9;
pub const EVENT_COUNT: usize = 160;
pub const DEGRADE_PROMILLE: u32 = 400; // 单类权值占比 > 40% 触发降级标注
pub const TOTAL_PROMILLE: u32 = 1000;

pub fn sig_name(k: u8) -> &'static str {
    match k {
        SIG_RETRY_STORM => "retry-storm",
        SIG_RAGE_CLICK => "rage-click",
        SIG_DEAD_CLICK => "dead-click",
        SIG_TIMEOUT => "timeout",
        SIG_BACKNAV_LOOP => "backnav-loop",
        _ => "undefined",
    }
}

/// 确定性 LCG。
pub struct Lcg {
    pub state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg {
            state: seed.wrapping_add(0x517C_C1B7_2722_0A95),
        }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.state >> 33) as u32
    }
    pub fn below(&mut self, bound: usize) -> usize {
        (self.next_u32() as usize) % bound
    }
}

/// 挫败信号事件。
#[derive(Clone, Debug, PartialEq)]
pub struct FrustEvent {
    pub session: u32,   // 会话号，非零
    pub kind: u8,       // 五类闭集
    pub intensity: u8,  // 1..=9
    pub ts_ms: u32,     // 段内相对毫秒，单调
}

impl FrustEvent {
    pub fn kind_legal(&self) -> bool {
        (self.kind as usize) < SIG_KINDS
    }
    pub fn intensity_legal(&self) -> bool {
        self.intensity >= 1 && self.intensity <= INTENSITY_MAX
    }
}

/// 段报表：五类计数与权值并账。
#[derive(Debug)]
pub struct SegmentReport {
    pub counts: [usize; SIG_KINDS],
    pub weights: [u64; SIG_KINDS],
    pub total_weight: u64,
    pub degraded: [bool; SIG_KINDS],
}

impl SegmentReport {
    pub fn new() -> SegmentReport {
        SegmentReport {
            counts: [0usize; SIG_KINDS],
            weights: [0u64; SIG_KINDS],
            total_weight: 0,
            degraded: [false; SIG_KINDS],
        }
    }
    pub fn absorb(&mut self, e: &FrustEvent) {
        let k = e.kind as usize;
        self.counts[k] += 1;
        self.weights[k] += e.intensity as u64;
        self.total_weight += e.intensity as u64;
    }
    /// 占比（千分位）并落降级标注：确定性比较，无浮点。
    pub fn settle_degrade(&mut self) {
        if self.total_weight == 0 {
            return;
        }
        let mut k = 0usize;
        while k < SIG_KINDS {
            let promille = (self.weights[k] * TOTAL_PROMILLE as u64) / self.total_weight;
            self.degraded[k] = promille > DEGRADE_PROMILLE as u64;
            k += 1;
        }
    }
    pub fn balance(&self) -> u64 {
        let mut t: u64 = 0;
        for w in self.weights.iter() {
            t += *w;
        }
        t
    }
}

/// 事件流指纹（复跑一致性）。
pub fn stream_fp(events: &[FrustEvent]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for e in events.iter() {
        h ^= (e.session as u64) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= ((e.session >> 8) as u64) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= e.kind as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= e.intensity as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= ((e.ts_ms >> 16) as u64) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= (e.ts_ms as u64) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 生成确定性挫败信号流：时间戳严格递增，强度 1..=9。
pub fn gen_stream(seed: u64) -> Vec<FrustEvent> {
    let mut rng = Lcg::new(seed);
    let mut events: Vec<FrustEvent> = Vec::new();
    let mut ts: u32 = 1000;
    let mut session_ctr: u32 = 700;
    while events.len() < EVENT_COUNT {
        let kind = rng.below(SIG_KINDS) as u8;
        let intensity = 1 + (rng.below(INTENSITY_MAX as usize) as u8);
        ts += 25 + (rng.below(400) as u32);
        if rng.below(3) == 0 {
            session_ctr += 1;
        }
        let e = FrustEvent {
            session: session_ctr,
            kind,
            intensity,
            ts_ms: ts,
        };
        events.push(e);
    }
    events
}

/// Top-N 重演：按权值降序取前 N 类下标（稳定并列按类号升序）。
pub fn topn_kinds(rep: &SegmentReport, n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::new();
    let mut k = 0usize;
    while k < SIG_KINDS {
        order.push(k);
        k += 1;
    }
    // 插入排序：权重降序、类号升序，确定性无浮点。
    let mut i = 1usize;
    while i < order.len() {
        let mut j = i;
        while j > 0 {
            let a = order[j - 1];
            let b = order[j];
            let wa = rep.weights[a];
            let wb = rep.weights[b];
            if wb > wa || (wb == wa && b < a) {
                order[j - 1] = b;
                order[j] = a;
                j -= 1;
            } else {
                break;
            }
        }
        i += 1;
    }
    order.truncate(n);
    order
}

/// 主巡检入口：R1..R8 场景，任一判据不满足返回 Err(诊断码)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 事件流结构：类型闭集、强度闭集、会话号非零。
    let events = gen_stream(0xF483_8601);
    if events.len() != EVENT_COUNT {
        return Err(E_REPORT_EMPTY);
    }
    for e in events.iter() {
        if !e.kind_legal() {
            return Err(E_TYPE_UNDEF);
        }
        if !e.intensity_legal() {
            return Err(E_INTENSITY);
        }
        if e.session == 0 {
            return Err(E_SESSION_ZERO);
        }
    }
    hits += 1;

    // R2 时间戳单调不降。
    let mut i = 1usize;
    while i < events.len() {
        if events[i].ts_ms < events[i - 1].ts_ms {
            return Err(E_TS_ORDER);
        }
        i += 1;
    }
    hits += 1;

    // R3 去重闸：同会话同类型同时间戳重复事件计数为零。
    let mut dup = 0usize;
    let mut a = 0usize;
    while a < events.len() {
        let mut b = a + 1;
        while b < events.len() {
            if events[a].session == events[b].session
                && events[a].kind == events[b].kind
                && events[a].ts_ms == events[b].ts_ms
            {
                dup += 1;
            }
            b += 1;
        }
        a += 1;
    }
    if dup != 0 {
        return Err(E_DUP_EVT);
    }
    hits += 1;

    // R4 段报表并账：五类计数和 == 全量事件数；权值账 == 全量强度和。
    let mut rep = SegmentReport::new();
    for e in events.iter() {
        rep.absorb(e);
    }
    let mut count_sum = 0usize;
    for c in rep.counts.iter() {
        count_sum += *c;
    }
    if count_sum != events.len() {
        return Err(E_BAL);
    }
    let mut src_weight: u64 = 0;
    for e in events.iter() {
        src_weight += e.intensity as u64;
    }
    if rep.balance() != src_weight || rep.total_weight != src_weight {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 占比闭窗：五类千分比之和为 1000（截断误差 ≤ 类数）。
    let mut prom_sum: u64 = 0;
    let mut k = 0usize;
    while k < SIG_KINDS {
        if rep.total_weight == 0 {
            return Err(E_REPORT_EMPTY);
        }
        prom_sum += (rep.weights[k] * TOTAL_PROMILLE as u64) / rep.total_weight;
        k += 1;
    }
    if prom_sum + (SIG_KINDS as u64) < TOTAL_PROMILLE as u64 {
        return Err(E_RATIO);
    }
    if prom_sum > TOTAL_PROMILLE as u64 {
        return Err(E_RATIO);
    }
    hits += 1;

    // R6 降级标注确定性：settle 后重演一遍，标注逐位相同。
    rep.settle_degrade();
    let mut rep2 = SegmentReport::new();
    for e in events.iter() {
        rep2.absorb(e);
    }
    rep2.settle_degrade();
    let mut k = 0usize;
    while k < SIG_KINDS {
        if rep.degraded[k] != rep2.degraded[k] {
            return Err(E_DEGRADE);
        }
        // 一致性推论：降级位必与占比阈值重算结果吻合。
        let promille = (rep.weights[k] * TOTAL_PROMILLE as u64) / rep.total_weight;
        if rep.degraded[k] != (promille > DEGRADE_PROMILLE as u64) {
            return Err(E_DEGRADE);
        }
        k += 1;
    }
    hits += 1;

    // R7 Top-N 重演：前 3 类与手工降序一致；N 越类数时截断安全。
    let top = topn_kinds(&rep, 3);
    if top.len() != 3 {
        return Err(E_TOPN);
    }
    let w0 = rep.weights[top[0]];
    let w1 = rep.weights[top[1]];
    let w2 = rep.weights[top[2]];
    if !(w0 >= w1 && w1 >= w2) {
        return Err(E_TOPN);
    }
    let full = topn_kinds(&rep, SIG_KINDS + 5);
    if full.len() != SIG_KINDS {
        return Err(E_TOPN);
    }
    let mut m = 1usize;
    while m < full.len() {
        let a = full[m - 1];
        let b = full[m];
        if rep.weights[b] > rep.weights[a] {
            return Err(E_TOPN);
        }
        if rep.weights[a] == rep.weights[b] && b < a {
            return Err(E_TOPN);
        }
        m += 1;
    }
    hits += 1;

    // R8 复跑指纹一致：重建事件流，流指纹与报表逐项相同。
    let rerun = gen_stream(0xF483_8601);
    if stream_fp(&events) != stream_fp(&rerun) {
        return Err(E_RERUN);
    }
    let mut rep3 = SegmentReport::new();
    for e in rerun.iter() {
        rep3.absorb(e);
    }
    let mut k = 0usize;
    while k < SIG_KINDS {
        if rep3.counts[k] != rep.counts[k] || rep3.weights[k] != rep.weights[k] {
            return Err(E_RERUN);
        }
        k += 1;
    }
    hits += 1;

    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::drill;
    #[test]
    fn drill_all_green() {
        assert!(drill().is_ok());
    }
}
