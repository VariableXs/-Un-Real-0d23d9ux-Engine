//! unx48327_smartperf — UNX-F48327 智能面性能预算（联想/纠错各 ≤8ms 采样，试产校准定）（诊断段 0xC9C0）。
//!
//! 判据原文：
//!   J1 10^4 采样 P99 落账；判据锚定 §AI-61 §5 J-1。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双通道预算闭集：联想/纠错各 ≤8ms（预算值入册即冻结，试产
//!      校准定，量产期不得暗中放宽——预算漂移即红线失守）。
//!   2. 采样确定性（J1 10^4）：耗时样本由 LCG 生成（99.5% 快径
//!      1..=8ms、0.5% 慢径 9..=16ms 的受控双峰），同种子复跑直方
//!      图逐位一致——性能账是可复现实验。
//!   3. P99 口径：升序第 ⌈q·n⌉ 位（q=0.99），直方图累计求位，与
//!      独立重算双路径对账；P99 超预算即红（不是均值不是 P50，
//!      尾延迟才是体验红线）。
//!   4. 直方图守恒：Σ桶计数 == 采样总数，逐通道账分立。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC9C0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CH_BAD: u16 = SEG_BASE + 1;   // 通道越闭集
const E_P99_OVER: u16 = SEG_BASE + 2; // P99 超预算
const E_HIST_BAD: u16 = SEG_BASE + 3; // 直方图不守恒
const E_FROZEN: u16 = SEG_BASE + 4;   // 预算已冻结拒改
const E_NO_SAMPLE: u16 = SEG_BASE + 5;// 零采样拒算
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 双通道闭集。
pub const CH_ASSOC: u8 = 0; // 联想
pub const CH_FIX: u8 = 1;   // 纠错
/// 预算（ms，试产校准冻结值）。
pub const BUDGET_MS: u32 = 8;
/// 采样总数（J1 10^4）。
pub const SAMPLE_N: u32 = 10000;
/// 耗时值域 1..=16ms。
pub const LAT_MAX: u32 = 16;

/// 校准标注四元（试产校准定，入册冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CalibMark {
    pub channel: u8,
    pub budget_ms: u32,
    pub calib_round: u32,
    pub frozen: bool,
}

/// 通道性能账：直方图 + 采样账 + 校准标注。
pub struct ChanPerf {
    pub channel: u8,
    hist: [u32; (LAT_MAX + 1) as usize], // 下标 0 弃用，1..=16
    pub sample_total: u32,
    pub calib: CalibMark,
}

impl ChanPerf {
    pub fn new(channel: u8, calib_round: u32) -> Result<ChanPerf, u16> {
        if channel > CH_FIX {
            return Err(E_CH_BAD);
        }
        Ok(ChanPerf {
            channel,
            hist: [0; (LAT_MAX + 1) as usize],
            sample_total: 0,
            calib: CalibMark {
                channel,
                budget_ms: BUDGET_MS,
                calib_round,
                frozen: true,
            },
        })
    }

    /// 单样本入账（毫秒，闭集 1..=16）。
    pub fn record(&mut self, ms: u32) -> Result<(), u16> {
        if ms == 0 || ms > LAT_MAX {
            return Err(E_HIST_BAD);
        }
        self.hist[ms as usize] += 1;
        self.sample_total += 1;
        Ok(())
    }

    /// 直方图守恒。
    pub fn hist_ok(&self) -> Result<(), u16> {
        let mut s: u32 = 0;
        let mut i = 1usize;
        while i <= LAT_MAX as usize {
            s += self.hist[i];
            i += 1;
        }
        if s != self.sample_total {
            return Err(E_HIST_BAD);
        }
        Ok(())
    }

    /// 分位数（升序第 ⌈q·n⌉ 位，直方图累计求位）。
    pub fn quantile(&self, q_permille: u32) -> Result<u32, u16> {
        if self.sample_total == 0 {
            return Err(E_NO_SAMPLE);
        }
        let rank = (self.sample_total * q_permille + 999) / 1000; // ⌈q·n⌉
        let mut acc: u32 = 0;
        let mut i = 1usize;
        while i <= LAT_MAX as usize {
            acc += self.hist[i];
            if acc >= rank {
                return Ok(i as u32);
            }
            i += 1;
        }
        Ok(LAT_MAX)
    }

    pub fn p99(&self) -> Result<u32, u16> {
        self.quantile(990)
    }

    /// 预算判定：P99 ≤ 冻结预算。
    pub fn budget_ok(&self) -> Result<(), u16> {
        let p = self.p99()?;
        if p > self.calib.budget_ms {
            return Err(E_P99_OVER);
        }
        Ok(())
    }

    /// 预算改写：冻结态拒改（红线），未冻结方可改。
    pub fn set_budget(&mut self, ms: u32) -> Result<(), u16> {
        if self.calib.frozen {
            return Err(E_FROZEN);
        }
        self.calib.budget_ms = ms;
        Ok(())
    }

    /// 直方图指纹（复跑对照）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xC9C0_C9C1;
        let mut i = 1usize;
        while i <= LAT_MAX as usize {
            fp = fp.rotate_left(5) ^ (self.hist[i] as u64);
            i += 1;
        }
        fp
    }
}

/// 受控双峰采样：99.5% 走 1..=8ms 快径，0.5% 走 9..=16ms 慢径。
pub fn sample_ms(lcg: &mut Lcg) -> u32 {
    let gate = lcg.next_below(200);
    if gate < 199 {
        (lcg.next_below(8) + 1) as u32
    } else {
        (lcg.next_below(8) + 9) as u32
    }
}

/// 跑满一通道 ×10^4 采样并入账。
pub fn run_channel(cp: &mut ChanPerf, seed: u64) -> Result<(), u16> {
    let mut lcg = Lcg::new(seed)?;
    let mut n = 0u32;
    while n < SAMPLE_N {
        cp.record(sample_ms(&mut lcg))?;
        n += 1;
    }
    Ok(())
}

/// 独立重算：不落账直接数第 ⌈q·n⌉ 位样本值（第二路径）。
pub fn recount_quantile(seed: u64, q_permille: u32) -> Result<u32, u16> {
    let mut lcg = Lcg::new(seed)?;
    let mut vals: Vec<u32> = Vec::new();
    let mut n = 0u32;
    while n < SAMPLE_N {
        let mut v = sample_ms(&mut lcg);
        if v == 0 || v > LAT_MAX {
            v = LAT_MAX;
        }
        vals.push(v);
        n += 1;
    }
    // 计数排序（值域 1..=16）。
    let mut cnt = [0u32; 17];
    let mut i = 0usize;
    while i < vals.len() {
        cnt[vals[i] as usize] += 1;
        i += 1;
    }
    let rank = (SAMPLE_N * q_permille + 999) / 1000;
    let mut acc: u32 = 0;
    let mut v = 1u32;
    while v <= LAT_MAX {
        acc += cnt[v as usize];
        if acc >= rank {
            return Ok(v);
        }
        v += 1;
    }
    Ok(LAT_MAX)
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_CH_BAD);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// 自例程判据：逐条落实 J1（10^4 采样 P99 落账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let seeds: [u64; 2] = [0xC9C1, 0xC9C2];

    // R1 双通道 ×10^4 采样 P99 落账：联想/纠错各跑满 10^4，P99
    //    ≤ 8ms 预算，直方图守恒。
    let mut assoc = ChanPerf::new(CH_ASSOC, 1)?;
    let mut fix = ChanPerf::new(CH_FIX, 1)?;
    run_channel(&mut assoc, seeds[0])?;
    run_channel(&mut fix, seeds[1])?;
    assoc.hist_ok()?;
    fix.hist_ok()?;
    if assoc.sample_total != SAMPLE_N || fix.sample_total != SAMPLE_N {
        return Err(E_HIST_BAD);
    }
    assoc.budget_ok()?;
    fix.budget_ok()?;
    hits += 1;

    // R2 P99 独立重算双路径对账：直方图求位 vs 计数排序重算，两
    //    通道 ×两分位（P99/P50）逐位一致。
    let mut assoc = ChanPerf::new(CH_ASSOC, 1)?;
    run_channel(&mut assoc, seeds[0])?;
    if assoc.p99()? != recount_quantile(seeds[0], 990)? {
        return Err(E_REPLAY_DIFF);
    }
    if assoc.quantile(500)? != recount_quantile(seeds[0], 500)? {
        return Err(E_REPLAY_DIFF);
    }
    let mut fix = ChanPerf::new(CH_FIX, 1)?;
    run_channel(&mut fix, seeds[1])?;
    if fix.p99()? != recount_quantile(seeds[1], 990)? {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R3 校准四元标注：双通道预算均 8ms 且冻结，通道号各自入册。
    let a = ChanPerf::new(CH_ASSOC, 3)?;
    let f = ChanPerf::new(CH_FIX, 3)?;
    if a.calib.budget_ms != BUDGET_MS || f.calib.budget_ms != BUDGET_MS {
        return Err(E_FROZEN);
    }
    if !a.calib.frozen || !f.calib.frozen {
        return Err(E_FROZEN);
    }
    if a.calib.channel != CH_ASSOC || f.calib.channel != CH_FIX {
        return Err(E_CH_BAD);
    }
    hits += 1;

    // R4 冻结红线：冻结态改预算必拒；可红路径——模拟超预算通道
    //    （预算改 2 不可走，用独立小账本灌慢样本后 budget_ok 必红）。
    let mut cp = ChanPerf::new(CH_ASSOC, 1)?;
    if cp.set_budget(16) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    let mut bad = ChanPerf::new(CH_FIX, 1)?;
    let mut lcg = Lcg::new(0xC9C9)?;
    let mut n = 0u32;
    while n < 200 {
        bad.record((lcg.next_below(8) + 9) as u32)?; // 全慢径
        n += 1;
    }
    if bad.budget_ok().is_ok() {
        return Err(E_P99_OVER); // 尾延迟超限必红（判据非恒绿）
    }
    hits += 1;

    // R5 边界闭集：零采样拒算分位；耗时 0/17 越值域拒收；通道越
    //    闭集拒建。
    let cp = ChanPerf::new(CH_ASSOC, 1)?;
    if cp.p99() != Err(E_NO_SAMPLE) {
        return Err(E_NO_SAMPLE);
    }
    let mut cp2 = ChanPerf::new(CH_FIX, 1)?;
    if cp2.record(0) != Err(E_HIST_BAD) || cp2.record(17) != Err(E_HIST_BAD) {
        return Err(E_HIST_BAD);
    }
    if cp2.sample_total != 0 {
        return Err(E_HIST_BAD);
    }
    match ChanPerf::new(7, 1) {
        Err(E_CH_BAD) => {}
        _ => return Err(E_CH_BAD),
    }
    hits += 1;

    // R6 可红演练：伪造直方图（破坏守恒）hist_ok 必红；指纹对伪
    //    改敏感。
    let mut cp = ChanPerf::new(CH_ASSOC, 1)?;
    cp.record(3)?;
    cp.record(5)?;
    cp.hist_ok()?;
    let fp0 = cp.fingerprint();
    cp.hist[3] += 1; // 私账改痕（同模块内可见，模拟账面篡改）
    if cp.hist_ok().is_ok() {
        return Err(E_HIST_BAD);
    }
    if cp.fingerprint() == fp0 {
        return Err(E_HIST_BAD);
    }
    hits += 1;

    // R7 双账分立：两通道直方图指纹不同（各自种子，不互串）。
    let mut a = ChanPerf::new(CH_ASSOC, 1)?;
    let mut f = ChanPerf::new(CH_FIX, 1)?;
    run_channel(&mut a, seeds[0])?;
    run_channel(&mut f, seeds[1])?;
    if a.fingerprint() == f.fingerprint() {
        return Err(E_REPLAY_DIFF);
    }
    a.hist_ok()?;
    f.hist_ok()?;
    hits += 1;

    // R8 复跑指纹一致与写回（判据 J2）：同种子第二轮复跑，直方图
    //    指纹逐位一致（可复现）；写回过。
    let mut a = ChanPerf::new(CH_ASSOC, 1)?;
    run_channel(&mut a, seeds[0])?;
    let fp1 = a.fingerprint();
    let mut a2 = ChanPerf::new(CH_ASSOC, 1)?;
    run_channel(&mut a2, seeds[0])?;
    if a2.fingerprint() != fp1 {
        return Err(E_REPLAY_DIFF);
    }
    a.budget_ok()?;
    if a.writeback_probe(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    a.writeback_probe(true, 0x1A)?;
    hits += 1;

    Ok(hits)
}

impl ChanPerf {
    /// 完成写回（判据 J2）。
    pub fn writeback_probe(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.sample_total == 0 {
            return Err(E_NO_SAMPLE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}
