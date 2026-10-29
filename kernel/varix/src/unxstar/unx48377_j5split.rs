//! unx48377_j5split — UNX-F48377 B19 批与 J5 通道分流声明（体验日志与审计流双通道不混流）（诊断段 0xD000）。
//!
//! 判据原文：
//!   J1 分流 ×10 抽查零混流；判据锚定 §AI-61 §8。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双通道声明：事件只有两条合法去向——体验日志（tag=0）与审计流
//!      （tag=1）；路由按 tag 判别位一次性定型，路由后再看 tag 改道
//!      一律禁止；tag ∉ {0,1} 的事件必须被拒之门外（E_ROUTE_BAD）。
//!   2. 零混流：×10 抽查批全量落渠后，逐条扫渠——体验渠道内不得有
//!      tag=1 事件，审计渠道内不得有 tag=0 事件；一条混流即全批报废。
//!   3. 渠内有序：路由不重排，各渠道内 ts 必须保持投喂序严格递增；
//!      渠道指纹（FNV 折叠）只由渠内事件序决定，同批重路由必同指纹。
//!   4. 零丢失：全量事件 id 全域唯一（1..N 连续编号），分流后两渠道
//!      条数之和恰等于投喂总数，id 点名一个不缺、一个不重。
//!   5. 检混能力：分流器必须能检出人工注入的错渠事件（假阴性为零），
//!      对干净渠道复检不得误报（假阳性为零）——检不出混流的分流器
//!      不配发声明。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xD000;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_ROUTE_BAD: u16 = SEG_BASE + 1;    // 非法 tag 拒收失效
const E_MIX_FLOW: u16 = SEG_BASE + 2;     // 渠道混流
const E_TS_ORDER: u16 = SEG_BASE + 3;     // 渠内 ts 非严格递增
const E_ID_LOSS: u16 = SEG_BASE + 4;      // id 丢失/重号/条数不符
const E_DETECT_MISS: u16 = SEG_BASE + 5;  // 错渠注入未检出（假阴性）
const E_FP_POSITIVE: u16 = SEG_BASE + 6;  // 干净复检误报（假阳性）
const E_REPLAY_DRIFT: u16 = SEG_BASE + 7; // 同批重路由指纹漂移
const E_CODE_RANGE: u16 = SEG_BASE + 8;   // 诊断码越段

/// 抽查批数（判据 ×10）。
pub const BATCHES: u32 = 10;

/// 每批事件数。
pub const PER_BATCH: usize = 16;

/// 合法去向。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    /// 体验日志（tag=0）。
    Experience,
    /// 审计流（tag=1）。
    Audit,
}

/// 待分流事件：id 全域唯一，tag 判别位只有 0/1 合法。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SplitEvent {
    pub id: u32,
    pub ts: u64,
    pub tag: u8,
}

/// tag → 渠道：一次判别定型；其余 tag 拒收。
pub fn channel_of(tag: u8) -> Result<Channel, u16> {
    match tag {
        0 => Ok(Channel::Experience),
        1 => Ok(Channel::Audit),
        _ => Err(E_ROUTE_BAD),
    }
}

/// FNV-1a 折叠（渠道指纹）。
pub fn fnv1a(seed: u64, x: u64) -> u64 {
    let mut h = seed;
    h ^= x;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// 渠道指纹：由 (id, ts) 投喂序折叠——同序同指纹，异序必异纹。
pub fn channel_fp(ch: &[SplitEvent]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < ch.len() {
        h = fnv1a(h, ch[i].id as u64);
        h = fnv1a(h, ch[i].ts);
        i += 1;
    }
    h
}

/// 渠道纯度扫描：渠内任一事件 tag 越轨即混流。
pub fn purity_scan(ch: &[SplitEvent], expect: u8) -> Result<(), u16> {
    let mut i = 0;
    while i < ch.len() {
        if ch[i].tag != expect {
            return Err(E_MIX_FLOW);
        }
        i += 1;
    }
    Ok(())
}

/// 渠内时序扫描：投喂序必须严格递增（路由不重排）。
pub fn ts_scan(ch: &[SplitEvent]) -> Result<(), u16> {
    let mut i = 1;
    while i < ch.len() {
        if ch[i].ts <= ch[i - 1].ts {
            return Err(E_TS_ORDER);
        }
        i += 1;
    }
    Ok(())
}

/// J5 分流器：双渠道落渠点。
pub struct Router {
    exp: Vec<SplitEvent>,
    aud: Vec<SplitEvent>,
}

impl Router {
    pub fn new() -> Self {
        Router { exp: Vec::new(), aud: Vec::new() }
    }

    /// 路由：按 tag 一次定型落渠；非法 tag 拒收不入渠。
    pub fn route(&mut self, e: SplitEvent) -> Result<Channel, u16> {
        let ch = channel_of(e.tag)?;
        match ch {
            Channel::Experience => self.exp.push(e),
            Channel::Audit => self.aud.push(e),
        }
        Ok(ch)
    }

    pub fn exp(&self) -> &[SplitEvent] {
        &self.exp
    }

    pub fn aud(&self) -> &[SplitEvent] {
        &self.aud
    }

    /// 双渠零混流全扫。
    pub fn purity(&self) -> Result<(), u16> {
        purity_scan(&self.exp, 0)?;
        purity_scan(&self.aud, 1)
    }

    /// 双渠时序全扫。
    pub fn ordered(&self) -> Result<(), u16> {
        ts_scan(&self.exp)?;
        ts_scan(&self.aud)
    }

    pub fn total(&self) -> usize {
        self.exp.len() + self.aud.len()
    }
}

/// 批夹具：批 b 的 16 条事件，id 全域连续（b*16+1 起），ts 批内递增；
/// tag 确定性均匀分布：tag=1（审计）当且仅当 i%4 == b%4（每批恰 4 审计 / 12 体验）。
pub fn batch_events(b: u32) -> Vec<SplitEvent> {
    let mut evs: Vec<SplitEvent> = Vec::new();
    let mut i = 0usize;
    while i < PER_BATCH {
        let tag = if i % 4 == (b as usize) % 4 { 1u8 } else { 0u8 };
        evs.push(SplitEvent {
            id: b * PER_BATCH as u32 + i as u32 + 1,
            ts: (b as u64) * 10_000 + i as u64,
            tag,
        });
        i += 1;
    }
    evs
}

/// 自例程判据：逐条落实 J1（分流 ×10 抽查零混流），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 ×10 批全量分流：条数之和恰 160，渠间分布恰 120/40。
    let mut router = Router::new();
    let mut b = 0u32;
    let mut exp_total = 0usize;
    let mut aud_total = 0usize;
    while b < BATCHES {
        let evs = batch_events(b);
        let mut i = 0usize;
        while i < evs.len() {
            let ch = router.route(evs[i])?;
            match ch {
                Channel::Experience => exp_total += 1,
                Channel::Audit => aud_total += 1,
            }
            i += 1;
        }
        b += 1;
    }
    if router.total() != (BATCHES as usize) * PER_BATCH {
        return Err(E_ID_LOSS);
    }
    if exp_total != 120 || aud_total != 40 {
        return Err(E_MIX_FLOW);
    }
    hits += 1;

    // R2 零混流全扫：×10 批落渠后逐条扫渠，任一 tag 越轨即废。
    router.purity()?;
    hits += 1;

    // R3 渠内有序：双渠 ts 严格递增（路由不重排的可证形态）。
    router.ordered()?;
    hits += 1;

    // R4 零丢失点名：id 1..=160 每号恰被点名一次，不缺不重。
    let mut seen: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k <= (BATCHES as usize) * PER_BATCH {
        seen.push(0u8);
        k += 1;
    }
    let mut i = 0usize;
    while i < router.exp().len() {
        let id = router.exp()[i].id as usize;
        if id == 0 || id > 160 {
            return Err(E_ID_LOSS);
        }
        if seen[id] != 0 {
            return Err(E_ID_LOSS); // 重号
        }
        seen[id] = 1;
        i += 1;
    }
    i = 0;
    while i < router.aud().len() {
        let id = router.aud()[i].id as usize;
        if id == 0 || id > 160 {
            return Err(E_ID_LOSS);
        }
        if seen[id] != 0 {
            return Err(E_ID_LOSS); // 重号
        }
        seen[id] = 1;
        i += 1;
    }
    let mut id = 1usize;
    while id <= 160 {
        if seen[id] != 1 {
            return Err(E_ID_LOSS); // 缺号
        }
        id += 1;
    }
    hits += 1;

    // R5 检混能力（假阴性为零）：向体验渠副本注入一条 tag=1 错渠事件，
    //    纯度扫描必须报警。
    let mut tampered: Vec<SplitEvent> = Vec::new();
    let mut i = 0usize;
    while i < router.exp().len() {
        tampered.push(router.exp()[i]);
        i += 1;
    }
    let rogue = SplitEvent { id: 999, ts: 5, tag: 1 };
    tampered.insert(5, rogue);
    match purity_scan(&tampered, 0) {
        Err(E_MIX_FLOW) => {}
        _ => return Err(E_DETECT_MISS),
    }
    hits += 1;

    // R6 干净复检（假阳性为零）：原始双渠再扫不得误报；非法 tag 必拒收。
    router.purity()?;
    match channel_of(2) {
        Err(E_ROUTE_BAD) => {}
        _ => return Err(E_ROUTE_BAD),
    }
    match channel_of(255) {
        Err(E_ROUTE_BAD) => {}
        _ => return Err(E_ROUTE_BAD),
    }
    hits += 1;

    // R7 同批重路由：全新分流器重放 ×10 批，双渠指纹逐位一致。
    let mut router2 = Router::new();
    let mut b = 0u32;
    while b < BATCHES {
        let evs = batch_events(b);
        let mut i = 0usize;
        while i < evs.len() {
            router2.route(evs[i])?;
            i += 1;
        }
        b += 1;
    }
    if channel_fp(router2.exp()) != channel_fp(router.exp()) {
        return Err(E_REPLAY_DRIFT);
    }
    if channel_fp(router2.aud()) != channel_fp(router.aud()) {
        return Err(E_REPLAY_DRIFT);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_ROUTE_BAD,
        E_MIX_FLOW,
        E_TS_ORDER,
        E_ID_LOSS,
        E_DETECT_MISS,
        E_FP_POSITIVE,
        E_REPLAY_DRIFT,
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
