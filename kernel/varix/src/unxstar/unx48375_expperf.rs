//! unx48375_expperf — UNX-F48375 B19 批性能账（日志路径内存上限采样，试产校准定）（诊断段 0xCFC0）。
//!
//! 判据原文：
//!   J1 落账带校准标注；判据锚定 附则五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三段流水：日志路径固定为 ingest（单条收流）→ batch（8 条聚批）
//!      → flush（8 批落账）三段；每段占用 = 驻留条数×单条字节 + 段底数，
//!      占用模型常数全域唯一，不许各段私设口径。
//!   2. 采样即账：每投喂一条事件，三段同步采样一次并记高水位；
//!      采样账必须完整（n 条事件 → 每段恰 n 个采样点），缺一点即废账。
//!   3. 校准上限（试产校准定）：三段上限 [1024, 1024, 4096] 字节，
//!      每段必须带非零校准标注 + 试产批号 0x0B19（B19 试产批）——
//!      无标注的上限不配入账；高水位越上限即 E_LIMIT_BREACH。
//!   4. 递推单调：投喂规模 n 递增时各段高水位非递减（新鲜流水线逐级
//!      重跑对比），占用模型若随规模反向收缩即模型错误。
//!   5. 重放一致：同规模同种子重跑，三段高水位逐位一致——性能账是
//!      确定性账，不许抖动。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0xCFC0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_LIMIT_BREACH: u16 = SEG_BASE + 1; // 采样高水位超校准上限
const E_SAMPLE_BAD: u16 = SEG_BASE + 2;   // 采样账缺失/不完整
const E_CALIB_MISS: u16 = SEG_BASE + 3;   // 校准标注缺失
const E_RECURSE_BAD: u16 = SEG_BASE + 4;  // 递推高水位反向收缩
const E_REPLAY_DRIFT: u16 = SEG_BASE + 5; // 同输入重采样漂移
const E_MARGIN_BAD: u16 = SEG_BASE + 6;   // 裕度记录异常
const E_STAGE_BAD: u16 = SEG_BASE + 7;    // 占用模型/阶段序错误
const E_CODE_RANGE: u16 = SEG_BASE + 8;   // 诊断码越段

/// 单条事件占用（字节，全域唯一常数）。
pub const PER_ENTRY: u64 = 24;

/// 段底数（字节）：ingest=16 / batch=64 / flush=128。
pub const STAGE_OH: [u64; 3] = [16, 64, 128];

/// 聚批粒度：每 8 条 ingest 聚 1 批；每 8 批聚 1 块落账。
pub const FAN_IN: u64 = 8;

/// 试产校准上限（字节）：ingest / batch / flush。
pub const CAL_LIMITS: [u64; 3] = [1024, 1024, 4096];

/// 试产批号（B19）。
pub const CAL_BATCH_TAG: u32 = 0x0B19;

/// 阶段序：0=ingest 1=batch 2=flush。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Ingest,
    Batch,
    Flush,
}

pub fn stage_index(s: Stage) -> usize {
    match s {
        Stage::Ingest => 0,
        Stage::Batch => 1,
        Stage::Flush => 2,
    }
}

/// 校准表：三段上限 + 逐段校准标注（试产校准定，标注非零才有效）。
#[derive(Clone, Copy)]
pub struct CalibTable {
    pub limits: [u64; 3],
    pub annot: [u32; 3],
    pub batch_tag: u32,
}

impl CalibTable {
    /// 试产校准标注：ingest=0x19A1 / batch=0x19A2 / flush=0x19A3。
    pub fn trial() -> Self {
        CalibTable {
            limits: CAL_LIMITS,
            annot: [0x19A1, 0x19A2, 0x19A3],
            batch_tag: CAL_BATCH_TAG,
        }
    }

    pub fn annotated(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.annot[i] == 0 {
                return false;
            }
            i += 1;
        }
        self.batch_tag != 0
    }
}

/// 日志路径流水线：三段驻留计数 + 逐段高水位 + 采样账。
pub struct Pipeline {
    fed: u64,
    resident: [u64; 3],
    high: [u64; 3],
    samples: [u64; 3],
}

impl Pipeline {
    pub fn new() -> Self {
        Pipeline {
            fed: 0,
            resident: [0; 3],
            high: [0; 3],
            samples: [0; 3],
        }
    }

    /// 段占用 = 驻留×单条 + 段底数。
    pub fn footprint(&self, s: usize) -> u64 {
        self.resident[s] * PER_ENTRY + STAGE_OH[s]
    }

    /// 投喂 count 条：事件先入 ingest，每 FAN_IN 条聚 1 批入 batch，
    /// 每 FAN_IN 批聚 1 块入 flush；每条事件后三段同步采样。
    pub fn feed(&mut self, count: u64) {
        let mut k = 0;
        while k < count {
            self.resident[0] += 1;
            self.fed += 1;
            if self.resident[0] % FAN_IN == 0 {
                self.resident[0] -= FAN_IN;
                self.resident[1] += 1;
                if self.resident[1] % FAN_IN == 0 {
                    self.resident[1] -= FAN_IN;
                    self.resident[2] += 1;
                }
            }
            let mut s = 0;
            while s < 3 {
                let f = self.footprint(s);
                self.samples[s] += 1;
                if f > self.high[s] {
                    self.high[s] = f;
                }
                s += 1;
            }
            k += 1;
        }
    }

    pub fn high(&self, s: usize) -> u64 {
        self.high[s]
    }

    pub fn samples(&self, s: usize) -> u64 {
        self.samples[s]
    }

    pub fn fed(&self) -> u64 {
        self.fed
    }
}

/// 上限核对：任一段高水位 > 校准上限即越限。
pub fn check_limits(high: &[u64; 3], limits: &[u64; 3]) -> Result<(), u16> {
    let mut s = 0;
    while s < 3 {
        if high[s] > limits[s] {
            return Err(E_LIMIT_BREACH);
        }
        s += 1;
    }
    Ok(())
}

/// 裕度记录：limit - high_water，三段全为正才配入账。
pub fn margins(high: &[u64; 3], limits: &[u64; 3]) -> Result<[u64; 3], u16> {
    let mut m = [0u64; 3];
    let mut s = 0;
    while s < 3 {
        if high[s] > limits[s] {
            return Err(E_MARGIN_BAD);
        }
        m[s] = limits[s] - high[s];
        s += 1;
    }
    Ok(m)
}

/// 自例程判据：逐条落实 J1（落账带校准标注），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 占用模型基准：n=8 精确点位——ingest 峰 7 条（7×24+16=184），
    //    batch 聚 1 批（1×24+64=88），flush 空转段底数 128。
    let mut p8 = Pipeline::new();
    p8.feed(8);
    if p8.high(0) != 7 * PER_ENTRY + 16 {
        return Err(E_STAGE_BAD);
    }
    if p8.high(1) != 1 * PER_ENTRY + 64 {
        return Err(E_STAGE_BAD);
    }
    if p8.high(2) != STAGE_OH[2] {
        return Err(E_STAGE_BAD);
    }
    if p8.fed() != 8 {
        return Err(E_STAGE_BAD);
    }
    hits += 1;

    // R2 n=512 全程越限核对：三段高水位 [184, 232, 320] 全 ≤ 校准上限
    //    （512 事件 → 64 批 → 两级扇入后 8 落账块，flush 驻留 8×24+128=320）。
    let mut p = Pipeline::new();
    p.feed(512);
    if p.high(0) != 184 || p.high(1) != 232 || p.high(2) != 8 * PER_ENTRY + 128 {
        return Err(E_STAGE_BAD);
    }
    let highs = [p.high(0), p.high(1), p.high(2)];
    check_limits(&highs, &CAL_LIMITS)?;
    hits += 1;

    // R3 采样账完整：n=512 → 每段恰 512 个采样点，一段都不许缺。
    let mut s = 0;
    while s < 3 {
        if p.samples(s) != 512 {
            return Err(E_SAMPLE_BAD);
        }
        s += 1;
    }
    hits += 1;

    // R4 校准标注：三段标注非零 + 试产批号 0x0B19（B19 试产校准定）。
    let cal = CalibTable::trial();
    if !cal.annotated() || cal.batch_tag != CAL_BATCH_TAG {
        return Err(E_CALIB_MISS);
    }
    let mut s = 0;
    while s < 3 {
        if cal.annot[s] != 0x19A1 + s as u32 {
            return Err(E_CALIB_MISS);
        }
        s += 1;
    }
    hits += 1;

    // R5 递推单调：n ∈ {64,128,256,512} 逐级重跑（新鲜流水线），
    //    各段高水位非递减——占用模型不许随规模反向收缩。
    let mut prev = [0u64; 3];
    let ns = [64u64, 128, 256, 512];
    let mut ni = 0usize;
    while ni < ns.len() {
        let mut pr = Pipeline::new();
        pr.feed(ns[ni]);
        let mut s = 0;
        while s < 3 {
            if pr.high(s) < prev[s] {
                return Err(E_RECURSE_BAD);
            }
            prev[s] = pr.high(s);
            s += 1;
        }
        ni += 1;
    }
    // flush 段高水位必须随规模真实增长（512 → 8 落账块驻留）。
    if prev[2] != 8 * PER_ENTRY + 128 {
        return Err(E_RECURSE_BAD);
    }
    hits += 1;

    // R6 重放一致：n=512 重跑，三段高水位与采样账逐位一致。
    let mut p2 = Pipeline::new();
    p2.feed(512);
    let mut s = 0;
    while s < 3 {
        if p2.high(s) != p.high(s) || p2.samples(s) != p.samples(s) {
            return Err(E_REPLAY_DRIFT);
        }
        s += 1;
    }
    hits += 1;

    // R7 越限检出 + 裕度：压低上限探针必报 E_LIMIT_BREACH；
    //    正常上限下三段裕度全为正且逐段可复算。
    let tight = [128u64, 128, 128];
    match check_limits(&highs, &tight) {
        Err(E_LIMIT_BREACH) => {}
        _ => return Err(E_LIMIT_BREACH),
    }
    let m = margins(&highs, &CAL_LIMITS)?;
    let mut s = 0;
    while s < 3 {
        if m[s] == 0 || m[s] != CAL_LIMITS[s] - highs[s] {
            return Err(E_MARGIN_BAD);
        }
        s += 1;
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 8] = [
        E_LIMIT_BREACH,
        E_SAMPLE_BAD,
        E_CALIB_MISS,
        E_RECURSE_BAD,
        E_REPLAY_DRIFT,
        E_MARGIN_BAD,
        E_STAGE_BAD,
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
