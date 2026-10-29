//! unx48376_expvec — UNX-F48376 B19 批向量账（体验事件用例集+校验和）（诊断段 0xCFE0）。
//!
//! 判据原文：
//!   J1 用例 ×20 可复跑；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 用例集封闭：向量账恰 20 条用例，每条用例 = 有序体验事件向量
//!      （ts 单调、kind ∈ 1..=4、val 有界），用例 id 严格递增无重号；
//!      用例由固定种子 LCG 生成，集内内容全域确定、零真随机。
//!   2. 校验和入账：每条用例折叠 FNV-1a 校验和入向量账；重算校验和
//!      必须与在账值逐位一致（×20 全对），任何一位漂移即废账。
//!   3. 可复跑：每条用例跑两遍状态折叠，两遍状态指纹逐位相等——
//!      复跑不是抽样，×20 条全部复跑，一条都不许跳。
//!   4. 篡改检出：任一用例事件被篡改一位，重算校验和必不等于在账值
//!      （假阴性为零）；对干净用例复检不得误报（假阳性为零）。
//!   5. 账目幂等：同 id 重登记不重记（账长不变）；×20 校验和两两互异，
//!      撞和即账目混淆，必须暴露。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCFE0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CASE_SHAPE: u16 = SEG_BASE + 1;  // 用例形违规（空向量/id 越界）
const E_CASE_ORDER: u16 = SEG_BASE + 2;  // id 非严格递增/重号
const E_EVENT_BAD: u16 = SEG_BASE + 3;   // 事件域违规（ts/kind/val）
const E_SUM_DRIFT: u16 = SEG_BASE + 4;   // 重算校验和与在账值漂移
const E_REPLAY_DRIFT: u16 = SEG_BASE + 5; // 复跑状态指纹漂移
const E_TAMPER_MISS: u16 = SEG_BASE + 6; // 篡改未检出
const E_FP_POSITIVE: u16 = SEG_BASE + 7; // 干净复检误报
const E_LEDGER_BAD: u16 = SEG_BASE + 8;  // 账目幂等/撞和违规
const E_CODE_RANGE: u16 = SEG_BASE + 9;  // 诊断码越段

/// 用例条数（判据 ×20）。
pub const CASE_N: u32 = 20;

/// 每用例事件数下限。
pub const MIN_EVENTS: usize = 8;

/// 确定性线性同余发生器（用例生成唯一随机源）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 体验事件：ts 单调、kind ∈ 1..=4、val ∈ 0..0xFFFF。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Event {
    pub ts: u64,
    pub kind: u8,
    pub val: u32,
}

impl Event {
    pub fn in_domain(&self) -> bool {
        self.kind >= 1 && self.kind <= 4 && self.val <= 0xFFFF
    }
}

/// 体验事件用例：id + 有序事件向量。
#[derive(Clone, PartialEq, Eq)]
pub struct UseCase {
    pub id: u32,
    pub events: Vec<Event>,
}

/// FNV-1a 折叠。
pub fn fnv1a(seed: u64, x: u64) -> u64 {
    let mut h = seed;
    h ^= x;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// 用例校验和：事件序列 FNV-1a 全折叠（混入 id 与事件数）。
pub fn checksum_of(c: &UseCase) -> u64 {
    let mut h = fnv1a(0xCBF2_9CE4_8422_2325, c.id as u64);
    h = fnv1a(h, c.events.len() as u64);
    let mut i = 0;
    while i < c.events.len() {
        h = fnv1a(h, c.events[i].ts);
        h = fnv1a(h, c.events[i].kind as u64);
        h = fnv1a(h, c.events[i].val as u64);
        i += 1;
    }
    h | (1 << 63) // 高位置位：校验和恒非零
}

/// 用例状态指纹：复跑折叠（加权累加 + FNV 混叠，两遍同构）。
pub fn state_hash(c: &UseCase) -> u64 {
    let mut h: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut acc: u64 = 0;
    let mut i = 0;
    while i < c.events.len() {
        acc = acc.wrapping_add(c.events[i].val as u64);
        h ^= c.events[i].ts ^ (acc & 0xFFFF);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= c.events[i].kind as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 用例形判据：非空、长度 ≥ MIN_EVENTS、id ∈ 1..、事件域合法、ts 严格递增。
pub fn case_shape(c: &UseCase) -> Result<(), u16> {
    if c.events.len() < MIN_EVENTS || c.id == 0 {
        return Err(E_CASE_SHAPE);
    }
    let mut i = 0;
    while i < c.events.len() {
        if !c.events[i].in_domain() {
            return Err(E_EVENT_BAD);
        }
        if i > 0 && c.events[i].ts <= c.events[i - 1].ts {
            return Err(E_EVENT_BAD);
        }
        i += 1;
    }
    Ok(())
}

/// 用例生成：id=i，长度 8+2*(i%5)，固定种子 LCG（种子混入 id）。
pub fn gen_case(i: u32) -> UseCase {
    let mut lc = Lcg::new((48376u32 << 8) ^ i.wrapping_mul(2_654_435_761));
    let len = MIN_EVENTS + 2 * (i % 5) as usize;
    let mut events: Vec<Event> = Vec::new();
    let mut ts: u64 = 1_000 + i as u64 * 10_000;
    let mut k = 0;
    while k < len {
        ts += 1 + (lc.next() % 900) as u64;
        let kind = (lc.next() % 4) as u8 + 1;
        let val = (lc.next() % 0xFFFF) as u32;
        events.push(Event { ts, kind, val });
        k += 1;
    }
    UseCase { id: i, events }
}

/// 向量账：用例 id → 校验和，append-only，同 id 重登记幂等不重记。
pub struct VecLedger {
    entries: Vec<(u32, u64)>,
}

impl VecLedger {
    pub fn new() -> Self {
        VecLedger { entries: Vec::new() }
    }

    /// 返回 true = 新记；false = 幂等重放；同 id 异和 = 账目冲突。
    pub fn register(&mut self, id: u32, sum: u64) -> Result<bool, u16> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == id {
                if self.entries[i].1 != sum {
                    return Err(E_LEDGER_BAD);
                }
                return Ok(false);
            }
            i += 1;
        }
        self.entries.push((id, sum));
        Ok(true)
    }

    pub fn get(&self, id: u32) -> Option<u64> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == id {
                return Some(self.entries[i].1);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 自例程判据：逐条落实 J1（用例 ×20 可复跑），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut cases: Vec<UseCase> = Vec::new();
    let mut i = 1u32;
    while i <= CASE_N {
        cases.push(gen_case(i));
        i += 1;
    }

    // R1 用例集封闭：恰 20 条，id 严格递增无重号，逐条形判据全过。
    if cases.len() != CASE_N as usize {
        return Err(E_CASE_SHAPE);
    }
    let mut ci = 1usize;
    while ci < cases.len() {
        if cases[ci].id <= cases[ci - 1].id {
            return Err(E_CASE_ORDER);
        }
        ci += 1;
    }
    let mut ci = 0usize;
    while ci < cases.len() {
        case_shape(&cases[ci])?;
        ci += 1;
    }
    hits += 1;

    // R2 校验和入账：×20 全登记；重算与在账逐位一致。
    let mut led = VecLedger::new();
    let mut ci = 0usize;
    while ci < cases.len() {
        let sum = checksum_of(&cases[ci]);
        led.register(cases[ci].id, sum)?;
        ci += 1;
    }
    if led.len() != CASE_N as usize {
        return Err(E_LEDGER_BAD);
    }
    let mut ci = 0usize;
    while ci < cases.len() {
        let expect = match led.get(cases[ci].id) {
            Some(v) => v,
            None => return Err(E_LEDGER_BAD),
        };
        if checksum_of(&cases[ci]) != expect {
            return Err(E_SUM_DRIFT);
        }
        ci += 1;
    }
    hits += 1;

    // R3 可复跑：×20 条各跑两遍状态折叠，指纹逐位相等。
    let mut ci = 0usize;
    while ci < cases.len() {
        let h1 = state_hash(&cases[ci]);
        let h2 = state_hash(&cases[ci]);
        if h1 != h2 {
            return Err(E_REPLAY_DRIFT);
        }
        ci += 1;
    }
    hits += 1;

    // R4 幂等登记：同 id 同和重登记不重记；同 id 异和必须暴露冲突。
    let mut ci = 0usize;
    while ci < cases.len() {
        let sum = checksum_of(&cases[ci]);
        let fresh = led.register(cases[ci].id, sum)?;
        if fresh {
            return Err(E_LEDGER_BAD);
        }
        ci += 1;
    }
    if led.len() != CASE_N as usize {
        return Err(E_LEDGER_BAD);
    }
    match led.register(1, checksum_of(&cases[0]) ^ 1) {
        Err(E_LEDGER_BAD) => {}
        _ => return Err(E_LEDGER_BAD),
    }
    hits += 1;

    // R5 撞和防线：×20 校验和两两互异，任一对撞和即账目混淆。
    let mut a = 0usize;
    while a < cases.len() {
        let mut b = a + 1;
        while b < cases.len() {
            if checksum_of(&cases[a]) == checksum_of(&cases[b]) {
                return Err(E_LEDGER_BAD);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R6 篡改检出（假阴性为零）：篡改第 7 条用例一位事件值，重算必漂移。
    let mut tam = cases[6].clone();
    tam.events[3].val ^= 0x0001;
    let reg = match led.get(tam.id) {
        Some(v) => v,
        None => return Err(E_LEDGER_BAD),
    };
    if checksum_of(&tam) == reg {
        return Err(E_TAMPER_MISS);
    }
    hits += 1;

    // R7 干净复检（假阳性为零）：未篡改用例重算必与在账一致。
    let mut ci = 0usize;
    while ci < cases.len() {
        let reg = match led.get(cases[ci].id) {
            Some(v) => v,
            None => return Err(E_LEDGER_BAD),
        };
        if checksum_of(&cases[ci]) != reg {
            return Err(E_FP_POSITIVE);
        }
        ci += 1;
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 9] = [
        E_CASE_SHAPE,
        E_CASE_ORDER,
        E_EVENT_BAD,
        E_SUM_DRIFT,
        E_REPLAY_DRIFT,
        E_TAMPER_MISS,
        E_FP_POSITIVE,
        E_LEDGER_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
