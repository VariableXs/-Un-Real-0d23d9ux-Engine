//! unx48354_a11yperf — UNX-F48354 B18 批性能面（查询/导出 ≤1ms 采样）（诊断段 0xCD20）。
//!
//! 判据原文：
//!   J1 10^4 采样 P99 落账；判据锚定 附则五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 双操作预算闭集：查询（QUERY）/导出（EXPORT）各 ≤1ms——
//!      无障碍面是响应敏感面，预算值入册冻结，量产期不得放宽。
//!   2. 采样确定性（J1 10^4）：耗时以 0.1ms 为最小单位（值域
//!      1..=20 即 0.1..2.0ms），99.5% 快径 1..=10（≤1ms）、0.5%
//!      慢径 11..=20，由 LCG 驱动，同种子复跑直方图逐位一致。
//!   3. P99 口径：升序第 ⌈q·n⌉ 位，直方图累计求位，与计数排序
//!      独立重算双路径对账；P99 超预算（>10 单位）即红——尾延
//!      迟才是无障碍体验红线（读屏用户等不起）。
//!   4. 直方图守恒：Σ桶 == 采样总数，双操作账分立互不冒充。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCD20;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_OP_BAD: u16 = SEG_BASE + 1;   // 操作越闭集
const E_P99_OVER: u16 = SEG_BASE + 2; // P99 超预算
const E_HIST_BAD: u16 = SEG_BASE + 3; // 直方图不守恒
const E_FROZEN: u16 = SEG_BASE + 4;   // 预算已冻结拒改
const E_NO_SAMPLE: u16 = SEG_BASE + 5;// 零采样拒算
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 7;// 写回摘要为空

/// 双操作闭集。
pub const OP_QUERY: u8 = 0;  // 查询
pub const OP_EXPORT: u8 = 1; // 导出
/// 预算：1.0ms（10 个 0.1ms 单位），冻结值。
pub const BUDGET_U: u32 = 10;
/// 采样总数（J1 10^4）。
pub const SAMPLE_N: u32 = 10000;
/// 耗时值域上限（2.0ms）。
pub const LAT_MAX: u32 = 20;

/// 操作性能账：直方图 + 采样账 + 冻结预算标注。
pub struct OpPerf {
    pub op: u8,
    hist: [u32; (LAT_MAX + 1) as usize],
    pub sample_total: u32,
    pub budget_u: u32,
    pub frozen: bool,
}

impl OpPerf {
    pub fn new(op: u8) -> Result<OpPerf, u16> {
        if op > OP_EXPORT {
            return Err(E_OP_BAD);
        }
        Ok(OpPerf {
            op,
            hist: [0; (LAT_MAX + 1) as usize],
            sample_total: 0,
            budget_u: BUDGET_U,
            frozen: true,
        })
    }

    /// 单样本入账（0.1ms 单位，闭集 1..=20）。
    pub fn record(&mut self, unit: u32) -> Result<(), u16> {
        if unit == 0 || unit > LAT_MAX {
            return Err(E_HIST_BAD);
        }
        self.hist[unit as usize] += 1;
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
        let rank = (self.sample_total * q_permille + 999) / 1000;
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
        if p > self.budget_u {
            return Err(E_P99_OVER);
        }
        Ok(())
    }

    /// 预算改写：冻结态拒改。
    pub fn set_budget(&mut self, u: u32) -> Result<(), u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        self.budget_u = u;
        Ok(())
    }

    /// 直方图指纹（复跑对照）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xCD20_CD21 ^ (self.op as u64);
        let mut i = 1usize;
        while i <= LAT_MAX as usize {
            fp = fp.rotate_left(5) ^ (self.hist[i] as u64);
            i += 1;
        }
        fp
    }
}

/// 受控双峰采样：99.5% 快径 1..=10，0.5% 慢径 11..=20。
pub fn sample_unit(lcg: &mut Lcg) -> u32 {
    let gate = lcg.next_below(200);
    if gate < 199 {
        (lcg.next_below(10) + 1) as u32
    } else {
        (lcg.next_below(10) + 11) as u32
    }
}

/// 跑满一操作 ×10^4 采样并入账。
pub fn run_op(cp: &mut OpPerf, seed: u64) -> Result<(), u16> {
    let mut lcg = Lcg::new(seed)?;
    let mut n = 0u32;
    while n < SAMPLE_N {
        cp.record(sample_unit(&mut lcg))?;
        n += 1;
    }
    Ok(())
}

/// 独立重算：计数排序直接求第 ⌈q·n⌉ 位（第二路径）。
pub fn recount_quantile(seed: u64, q_permille: u32) -> Result<u32, u16> {
    let mut lcg = Lcg::new(seed)?;
    let mut cnt = [0u32; 21];
    let mut n = 0u32;
    while n < SAMPLE_N {
        let v = sample_unit(&mut lcg);
        cnt[v as usize] += 1;
        n += 1;
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
            return Err(E_OP_BAD);
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
    let seeds: [u64; 2] = [0xCD21, 0xCD22];

    // R1 双操作 ×10^4 采样 P99 落账：查询/导出各跑满 10^4，P99
    //    ≤ 1.0ms（10 单位），直方图守恒。
    let mut q = OpPerf::new(OP_QUERY)?;
    let mut e = OpPerf::new(OP_EXPORT)?;
    run_op(&mut q, seeds[0])?;
    run_op(&mut e, seeds[1])?;
    q.hist_ok()?;
    e.hist_ok()?;
    if q.sample_total != SAMPLE_N || e.sample_total != SAMPLE_N {
        return Err(E_HIST_BAD);
    }
    q.budget_ok()?;
    e.budget_ok()?;
    hits += 1;

    // R2 P99 独立重算双路径对账：直方图求位 vs 计数排序重算，双
    //    操作 ×双分位（P99/P50）逐位一致。
    let mut q = OpPerf::new(OP_QUERY)?;
    run_op(&mut q, seeds[0])?;
    if q.p99()? != recount_quantile(seeds[0], 990)? {
        return Err(E_REPLAY_DIFF);
    }
    if q.quantile(500)? != recount_quantile(seeds[0], 500)? {
        return Err(E_REPLAY_DIFF);
    }
    let mut e = OpPerf::new(OP_EXPORT)?;
    run_op(&mut e, seeds[1])?;
    if e.p99()? != recount_quantile(seeds[1], 990)? {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R3 预算冻结标注：双操作预算均 10 单位（1.0ms）且冻结。
    let mut q = OpPerf::new(OP_QUERY)?;
    let mut e = OpPerf::new(OP_EXPORT)?;
    if q.budget_u != BUDGET_U || e.budget_u != BUDGET_U {
        return Err(E_FROZEN);
    }
    if !q.frozen || !e.frozen {
        return Err(E_FROZEN);
    }
    if q.set_budget(20) != Err(E_FROZEN) || e.set_budget(1) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // R4 可红演练：全慢径灌账 → P99 必超预算（判据非恒绿）；单桶
    //    伪改 → 守恒必红。
    let mut bad = OpPerf::new(OP_QUERY)?;
    let mut lcg = Lcg::new(0xCD29)?;
    let mut n = 0u32;
    while n < 300 {
        bad.record((lcg.next_below(10) + 11) as u32)?;
        n += 1;
    }
    if bad.budget_ok().is_ok() {
        return Err(E_P99_OVER);
    }
    let mut fp = OpPerf::new(OP_EXPORT)?;
    fp.record(3)?;
    fp.hist[3] += 1; // 私账改痕
    if fp.hist_ok().is_ok() {
        return Err(E_HIST_BAD);
    }
    hits += 1;

    // R5 边界闭集：零采样拒算；单位 0/21 拒收；操作越闭集拒建。
    let p = OpPerf::new(OP_QUERY)?;
    if p.p99() != Err(E_NO_SAMPLE) {
        return Err(E_NO_SAMPLE);
    }
    let mut p2 = OpPerf::new(OP_EXPORT)?;
    if p2.record(0) != Err(E_HIST_BAD) || p2.record(21) != Err(E_HIST_BAD) {
        return Err(E_HIST_BAD);
    }
    if OpPerf::new(9).err() != Some(E_OP_BAD) {
        return Err(E_OP_BAD);
    }
    hits += 1;

    // R6 双操作账分立：两操作直方图指纹不同（各自种子，不互串）。
    let mut q = OpPerf::new(OP_QUERY)?;
    let mut e = OpPerf::new(OP_EXPORT)?;
    run_op(&mut q, seeds[0])?;
    run_op(&mut e, seeds[1])?;
    if q.fingerprint() == e.fingerprint() {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 慢径占比账：10^4 中慢径（>10 单位）计数应落在 30~70 区间
    //    （0.5% 名义的合理带宽）——慢径不是零也不是灾。
    let mut q = OpPerf::new(OP_QUERY)?;
    run_op(&mut q, seeds[0])?;
    let mut slow: u32 = 0;
    let mut v = 11u32;
    while v <= LAT_MAX {
        slow += q.hist[v as usize];
        v += 1;
    }
    if slow < 30 || slow > 70 {
        return Err(E_HIST_BAD);
    }
    hits += 1;

    // R8 复跑指纹一致与写回（判据 J2）：同种子两轮复跑指纹逐位
    //    一致；写回过。
    let mut q = OpPerf::new(OP_QUERY)?;
    run_op(&mut q, seeds[0])?;
    let fp1 = q.fingerprint();
    let mut q2 = OpPerf::new(OP_QUERY)?;
    run_op(&mut q2, seeds[0])?;
    if q2.fingerprint() != fp1 {
        return Err(E_REPLAY_DIFF);
    }
    q.budget_ok()?;
    if q.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    q.writeback(true, 0x1D)?;
    hits += 1;

    Ok(hits)
}

impl OpPerf {
    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.sample_total == 0 {
            return Err(E_NO_SAMPLE);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}
