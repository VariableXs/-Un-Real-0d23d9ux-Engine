//! unx48374_expfuzz — UNX-F48374 B19 批 fuzz（随机事件流注入不崩不胀内存）（诊断段 0xCFA0）。
//!
//! 判据原文：
//!   J1 fuzz ×10^5 零崩零泄漏；判据锚定 体验十。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 确定性 fuzz：禁真随机，事件流由固定种子 LCG 驱动
//!      （x = x*1664525 + 1013904223，wrapping），×10^5 步全程可复跑，
//!      同种子重放必须逐位同迹——fuzz 结论才算账目级证据。
//!   2. 零崩：全部操作走 Result 通路，无 unwrap/expect/panic 面；
//!      注入操作含对抗极值（0 与 u64::MAX），任何一步都不允许崩溃路径。
//!   3. 零泄漏：日志环有界（cap=64），满则淘汰最旧；每步之后驻留
//!      ≤cap，全程高水位 ≤cap——内存只会在界内波动，绝不越界增长。
//!   4. 条目守恒：pushed == drained + 驻留 + 淘汰（含去重移除），
//!      恒等式在 fuzz 收尾逐项核对，任何一条事件都不许凭空消失或翻倍。
//!   5. 去重有效：重复注入的等值事件被就地去重收缩，驻留集合无重复；
//!      flush 后清空且二次 flush 幂等（0 条）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCFA0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_OP_REJECT: u16 = SEG_BASE + 1;   // 合法操作被拒（零崩判据）
const E_MEM_GROW: u16 = SEG_BASE + 2;    // 内存越界（零泄漏判据）
const E_CONSERVE: u16 = SEG_BASE + 3;    // 条目守恒恒等式破坏
const E_FLUSH_BAD: u16 = SEG_BASE + 4;   // flush 残留/二次 flush 非幂等
const E_DEDUP_BAD: u16 = SEG_BASE + 5;   // 去重失效
const E_REPLAY_DRIFT: u16 = SEG_BASE + 6; // 同种子重放漂移
const E_INJECT_MISS: u16 = SEG_BASE + 7; // 对抗注入未按界吸收
const E_CODE_RANGE: u16 = SEG_BASE + 8;  // 诊断码越段

/// fuzz 步数（判据 ×10^5）。
pub const FUZZ_STEPS: u32 = 100_000;

/// 日志环容量（有界内存硬界）。
pub const RING_CAP: usize = 64;

/// 确定性线性同余发生器（fuzz 唯一随机源，禁真随机）。
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

/// FNV-1a 全折叠（环内驻留序列指纹）。
pub fn fnv_stream(tokens: &[u64]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < tokens.len() {
        h ^= tokens[i];
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 有界日志环：满则淘汰最旧，全程 len ≤ cap。
pub struct RingLog {
    buf: Vec<u64>,
    cap: usize,
    high_water: usize,
    pushed: u64,
    drained: u64,
    evicted: u64,
}

impl RingLog {
    pub fn new(cap: usize) -> Self {
        RingLog {
            buf: Vec::new(),
            cap,
            high_water: 0,
            pushed: 0,
            drained: 0,
            evicted: 0,
        }
    }

    /// 入环：满则先淘汰最旧（有界内存的关键动作）。
    pub fn push(&mut self, v: u64) -> Result<(), u16> {
        if self.buf.len() >= self.cap {
            let _old = self.buf.remove(0);
            self.evicted += 1;
        }
        self.buf.push(v);
        self.pushed += 1;
        if self.buf.len() > self.high_water {
            self.high_water = self.buf.len();
        }
        if self.buf.len() > self.cap {
            return Err(E_MEM_GROW);
        }
        Ok(())
    }

    /// 出环一条：空环返回 Ok(None)（不是错误）。
    pub fn drain_one(&mut self) -> Result<Option<u64>, u16> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        let v = self.buf.remove(0);
        self.drained += 1;
        Ok(Some(v))
    }

    /// flush：清空环，返回清掉条数；空环 flush 幂等（0 条）。
    pub fn flush(&mut self) -> Result<usize, u16> {
        let n = self.buf.len();
        self.drained += n as u64;
        self.buf.clear();
        Ok(n)
    }

    /// 就地去重：等值事件只留一个，返回移除数（计入淘汰账）。
    pub fn dedup_scan(&mut self) -> Result<usize, u16> {
        let mut removed = 0usize;
        let mut i = 0;
        while i < self.buf.len() {
            let mut dup = false;
            let mut j = i + 1;
            while j < self.buf.len() {
                if self.buf[j] == self.buf[i] {
                    dup = true;
                    break;
                }
                j += 1;
            }
            if dup {
                let _ = self.buf.remove(i);
                removed += 1;
                self.evicted += 1;
            } else {
                i += 1;
            }
        }
        Ok(removed)
    }

    /// 守恒恒等式：pushed == drained + 驻留 + 淘汰。
    pub fn conserved(&self) -> bool {
        self.pushed == self.drained + self.buf.len() as u64 + self.evicted
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn high_water(&self) -> usize {
        self.high_water
    }

    pub fn counters(&self) -> (u64, u64, u64) {
        (self.pushed, self.drained, self.evicted)
    }

    pub fn contains(&self, v: u64) -> bool {
        let mut i = 0;
        while i < self.buf.len() {
            if self.buf[i] == v {
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn fingerprint(&self) -> u64 {
        fnv_stream(&self.buf)
    }
}

/// 操作数取值：混入对抗极值 0 与 u64::MAX（每 8 步必遇其一）。
pub fn operand(lc: &mut Lcg, step: u32) -> u64 {
    match lc.next() % 8 {
        0 => 0u64,
        1 => u64::MAX,
        2 => step as u64,
        _ => lc.next() as u64,
    }
}

/// 单步 fuzz：LCG 决定操作与操作数，全程 Result 通路，零 panic 面。
/// 返回操作类别（0 push / 1 drain / 2 dedup / 3 flush / 4 双推）。
pub fn fuzz_step(r: &mut RingLog, lc: &mut Lcg, step: u32) -> Result<u8, u16> {
    let op = lc.next() % 5;
    let x = operand(lc, step);
    match op {
        0 => {
            r.push(x)?;
            Ok(0)
        }
        1 => {
            let got = r.drain_one()?;
            match got {
                None | Some(_) => Ok(1),
            }
        }
        2 => {
            let _removed = r.dedup_scan()?;
            Ok(2)
        }
        3 => {
            let _n = r.flush()?;
            Ok(3)
        }
        _ => {
            r.push(x)?;
            r.push(x)?; // 双推制造等值重复，喂给去重器
            Ok(4)
        }
    }
}

/// 自例程判据：逐条落实 J1（fuzz ×10^5 零崩零泄漏），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10^5 步零崩：每步 Ok 通路完成，操作账齐全。
    let mut ring = RingLog::new(RING_CAP);
    let mut lc = Lcg::new(0x4837_4001);
    let mut op_count = [0u32; 5];
    let mut step = 0u32;
    while step < FUZZ_STEPS {
        let k = fuzz_step(&mut ring, &mut lc, step)?;
        op_count[k as usize] += 1;
        if ring.len() > RING_CAP {
            return Err(E_MEM_GROW);
        }
        step += 1;
    }
    let mut total_ops = 0u32;
    let mut oi = 0;
    while oi < 5 {
        total_ops += op_count[oi];
        oi += 1;
    }
    if total_ops != FUZZ_STEPS {
        return Err(E_OP_REJECT);
    }
    hits += 1;

    // R2 零泄漏：全程高水位 ≤ cap；收尾驻留 ≤ cap。
    if ring.high_water() > RING_CAP || ring.len() > RING_CAP {
        return Err(E_MEM_GROW);
    }
    hits += 1;

    // R3 条目守恒：pushed == drained + 驻留 + 淘汰。
    let (pushed, drained, evicted) = ring.counters();
    if !ring.conserved() || pushed != drained + ring.len() as u64 + evicted {
        return Err(E_CONSERVE);
    }
    if pushed == 0 {
        return Err(E_CONSERVE); // fuzz 必须真实注入了事件
    }
    hits += 1;

    // R4 flush 语义：满环 push 10 条 → flush 收 10 条、环空；二次 flush 幂等 0 条。
    let mut r2 = RingLog::new(RING_CAP);
    let mut n = 0u64;
    while n < 10 {
        r2.push(1_000 + n)?;
        n += 1;
    }
    let got = r2.flush()?;
    if got != 10 || r2.len() != 0 {
        return Err(E_FLUSH_BAD);
    }
    let got2 = r2.flush()?;
    if got2 != 0 || r2.len() != 0 {
        return Err(E_FLUSH_BAD);
    }
    hits += 1;

    // R5 去重语义：等值事件就地去重，驻留无重复，移除计入淘汰账。
    let mut r3 = RingLog::new(RING_CAP);
    r3.push(5)?;
    r3.push(5)?;
    r3.push(6)?;
    r3.push(5)?;
    r3.push(7)?;
    let removed = r3.dedup_scan()?;
    if removed != 2 || r3.len() != 3 {
        return Err(E_DEDUP_BAD);
    }
    if !r3.contains(5) || !r3.contains(6) || !r3.contains(7) {
        return Err(E_DEDUP_BAD);
    }
    r3.push(5)?; // 再注入等值重复
    let removed2 = r3.dedup_scan()?;
    if removed2 != 1 || r3.len() != 3 {
        return Err(E_DEDUP_BAD);
    }
    hits += 1;

    // R6 同种子重放：全新环同种子重跑 ×10^5，终态指纹与操作账逐位一致。
    let mut ring_b = RingLog::new(RING_CAP);
    let mut lc_b = Lcg::new(0x4837_4001);
    let mut op_count_b = [0u32; 5];
    step = 0;
    while step < FUZZ_STEPS {
        let k = fuzz_step(&mut ring_b, &mut lc_b, step)?;
        op_count_b[k as usize] += 1;
        step += 1;
    }
    if ring_b.fingerprint() != ring.fingerprint() {
        return Err(E_REPLAY_DRIFT);
    }
    let mut oi = 0;
    while oi < 5 {
        if op_count_b[oi] != op_count[oi] {
            return Err(E_REPLAY_DRIFT);
        }
        oi += 1;
    }
    hits += 1;

    // R7 对抗极值注入：0 与 u64::MAX 交替推 2×cap 次，界内吸收不越界，
    //    淘汰账恰等于溢出量（128 推 - 64 界 = 64 淘汰），守恒不破。
    let mut r4 = RingLog::new(RING_CAP);
    let mut n = 0u32;
    while n < (RING_CAP as u32) * 2 {
        let v = if n % 2 == 0 { u64::MAX } else { 0 };
        r4.push(v)?;
        n += 1;
    }
    if r4.len() != RING_CAP || r4.high_water() > RING_CAP {
        return Err(E_INJECT_MISS);
    }
    let (p4, d4, e4) = r4.counters();
    if p4 != (RING_CAP as u32 * 2) as u64 || e4 != RING_CAP as u64 || d4 != 0 {
        return Err(E_INJECT_MISS);
    }
    if !r4.conserved() {
        return Err(E_CONSERVE);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_OP_REJECT,
        E_MEM_GROW,
        E_CONSERVE,
        E_FLUSH_BAD,
        E_DEDUP_BAD,
        E_REPLAY_DRIFT,
        E_INJECT_MISS,
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
