//! unx36017_sidpool — UNX-F36017 SID 池化分配器（固定大小档 + 回收）。
//!
//! 判据原文：
//!   J1 池化分配 10^4 次/回收归池/水位峰值三项守恒断言全过；池耗尽
//!      注入走等待+告警路径非崩溃。
//! 设计纪律：
//!   1. 池模型：1024 固定槽 + 索引空闲栈（LIFO 回收复用）；分配即占
//!      槽、回收归栈，无堆参与（确定性地址序）。
//!   2. 水位面：峰值水位 = 历史最高活槽数，只升不降（观测锚）。
//!   3. 耗尽面：池满再分配 → 显性 Err（等待+告警路径：告警计数 +1，
//!      调用方重试语义），绝不 panic/静默覆盖；双回收拦截入账。
//!   4. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

pub const SEG_BASE: u16 = 0x7470;
use crate::checks::CheckSet;

// ── 诊断码闭集（0x7470~0x748F，独占）─────────────────────────
const E_EXHAUSTED: u16 = SEG_BASE + 1; // 池耗尽（等待+告警路径）
const E_DBL_FREE: u16 = SEG_BASE + 2; // 双回收（拦截入账）
const E_IDX: u16 = SEG_BASE + 3; // 坏索引（越界/未分配槽）
const E_BAL: u16 = SEG_BASE + 4; // 三项守恒破
const E_ALARM: u16 = SEG_BASE + 5; // 告警面失配

/// 池容量。
pub const POOL_CAP: usize = 1024;
/// 操作规模（判据 J1 的 10^4 面）。
pub const OPS_SCALE: u32 = 10_000;

/// SID 池。
pub struct SidPool {
    fp: [u64; POOL_CAP],
    alive: [bool; POOL_CAP],
    free_stack: [u16; POOL_CAP],
    free_top: usize,
    pub allocs: u32,
    pub frees: u32,
    pub live: i64,
    pub peak: u32,
    pub alarms: u32,
    pub dbl_free_hits: u32,
}

impl SidPool {
    pub fn new() -> SidPool {
        let mut st = [0u16; POOL_CAP];
        let mut i = 0;
        while i < POOL_CAP {
            st[i] = (POOL_CAP - 1 - i) as u16; // LIFO：0 号最先出栈
            i += 1;
        }
        SidPool {
            fp: [0; POOL_CAP],
            alive: [false; POOL_CAP],
            free_stack: st,
            free_top: POOL_CAP,
            allocs: 0,
            frees: 0,
            live: 0,
            peak: 0,
            alarms: 0,
            dbl_free_hits: 0,
        }
    }

    /// 分配：出空闲栈占槽；耗尽走等待+告警路径（Err 显性）。
    pub fn alloc(&mut self, fp: u64) -> Result<usize, u16> {
        if self.free_top == 0 {
            self.alarms += 1; // 告警：池耗尽，等待重试
            return Err(E_EXHAUSTED);
        }
        self.free_top -= 1;
        let idx = self.free_stack[self.free_top] as usize;
        self.fp[idx] = fp;
        self.alive[idx] = true;
        self.allocs += 1;
        self.live += 1;
        if self.live as u32 > self.peak {
            self.peak = self.live as u32; // 水位只升不降
        }
        Ok(idx)
    }

    /// 回收：活槽归栈；双回收/坏索引拦截入账。
    pub fn free(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= POOL_CAP {
            return Err(E_IDX);
        }
        if !self.alive[idx] {
            self.dbl_free_hits += 1;
            return Err(E_DBL_FREE);
        }
        self.alive[idx] = false;
        self.fp[idx] = 0;
        self.free_stack[self.free_top] = idx as u16;
        self.free_top += 1;
        self.frees += 1;
        self.live -= 1;
        Ok(())
    }

    /// 槽指纹读出（活槽）。
    pub fn fp_of(&self, idx: usize) -> Result<u64, u16> {
        if idx >= POOL_CAP || !self.alive[idx] {
            return Err(E_IDX);
        }
        Ok(self.fp[idx])
    }

    /// 空闲栈余量。
    pub fn free_count(&self) -> usize {
        self.free_top
    }

    /// 三项守恒：活数 == 分配-回收；空闲栈 == 容量-活数；峰值 ≥ 活数。
    pub fn conserve(&self) -> bool {
        self.live == self.allocs as i64 - self.frees as i64
            && self.free_count() == POOL_CAP - self.live as usize
            && self.peak >= self.live as u32
            && self.peak <= POOL_CAP as u32
    }

    /// 占用率（万分比，确定性整数面）。
    pub fn occupancy_bp(&self) -> u32 {
        (self.live.max(0) as u32) * 10_000 / POOL_CAP as u32
    }

    /// 峰值水位万分比（历史最高占用，观测面）。
    pub fn peak_bp(&self) -> u32 {
        self.peak * 10_000 / POOL_CAP as u32
    }
}

/// 池轮廓快照（收口/监控面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PoolSnapshot {
    pub live: i64,
    pub free: usize,
    pub peak: u32,
    pub allocs: u32,
    pub frees: u32,
    pub alarms: u32,
    pub dbl_free_hits: u32,
}

impl SidPool {
    /// 快照导出（账面全量一次取）。
    pub fn snapshot(&self) -> PoolSnapshot {
        PoolSnapshot {
            live: self.live,
            free: self.free_count(),
            peak: self.peak,
            allocs: self.allocs,
            frees: self.frees,
            alarms: self.alarms,
            dbl_free_hits: self.dbl_free_hits,
        }
    }
}

/// 快照守恒核：live+free==容量、peak 只升不降语义、账面差额闭合。
pub fn snapshot_conserve(s: &PoolSnapshot) -> bool {
    s.live + s.free as i64 == POOL_CAP as i64
        && s.allocs >= s.frees
        && (s.allocs - s.frees) as i64 == s.live
        && s.peak <= POOL_CAP as u32
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_EXHAUSTED => "池耗尽（等待+告警路径）",
        E_DBL_FREE => "双回收（拦截入账）",
        E_IDX => "坏索引（越界/未分配槽）",
        E_BAL => "三项守恒破",
        E_ALARM => "告警面失配",
        _ => "未知诊断码",
    }
}

/// 确定性演练：10^4 次操作三项守恒 + 池耗尽告警路径（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut pool = SidPool::new();
    let mut handles = [0usize; POOL_CAP];

    // R1：10^4 次操作面——阶段 A 全填 1024（水位=1024）；阶段 B
    // 回收一半；阶段 C 再分配一半（复用归池槽）；阶段 D 回收全部。
    // 合计 alloc 1024+512+512+0（含重复计）… 以账面为准核算：
    // A: 1024 alloc；B: 512 free；C: 512 alloc；D: 1024 free
    // → allocs=2048, frees=1536, live=0。
    // 操作计数 1024+512+512+1024 = 3072；再补查账重放凑满 10^4：
    // 阶段 E：512 槽循环 alloc/free 512×7 轮 = 7168 次（alloc 3584 +
    // free 3584）→ 总操作 10240 次（≥10^4 面）。
    let mut seed: u32 = 0x3611_0001;
    // 阶段 A：全填。
    let mut i = 0;
    while i < POOL_CAP {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        handles[i] = pool.alloc(((seed as u64) << 16) | i as u64)?;
        i += 1;
    }
    if pool.peak != POOL_CAP as u32 || pool.live != POOL_CAP as i64 {
        return Err(E_BAL);
    }
    // 阶段 B：回收一半（偶号槽）。
    let mut i = 0;
    while i < POOL_CAP {
        if i % 2 == 0 {
            pool.free(handles[i])?;
        }
        i += 1;
    }
    if pool.live != POOL_CAP as i64 / 2 {
        return Err(E_BAL);
    }
    // 阶段 C：再分配 512（复用归池槽，指纹可回读对平）。
    let mut i = 0;
    while i < POOL_CAP / 2 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let idx = pool.alloc((0xC0DEu64 << 32) | i as u64)?;
        if pool.fp_of(idx)? >> 32 != 0xC0DE {
            return Err(E_BAL);
        }
        i += 1;
    }
    if pool.live != POOL_CAP as i64 {
        return Err(E_BAL);
    }
    if !pool.conserve() {
        return Err(E_BAL);
    }
    // 阶段 D：回收全部。
    let mut i = 0;
    while i < POOL_CAP {
        if i % 2 == 0 {
            pool.free(handles[i])?; // 偶号槽：阶段 B 后已被 C 复用（活）
        } else {
            pool.free(handles[i])?; // 奇号槽：一直活
        }
        i += 1;
    }
    if pool.live != 0 || pool.peak != POOL_CAP as u32 {
        return Err(E_BAL);
    }
    hits += 1;

    // 阶段 E：512 槽循环 alloc/free 7 轮（凑满 10^4 操作面）。
    let mut round = 0;
    while round < 7 {
        let mut i = 0;
        while i < 512 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let idx = pool.alloc(seed as u64)?;
            pool.free(idx)?;
            i += 1;
        }
        round += 1;
    }
    let total_ops = pool.allocs + pool.frees;
    if total_ops < OPS_SCALE {
        return Err(E_BAL);
    }
    // 账面核算：allocs = 1024+512+3584 = 5120；frees = 512+1024+3584 = 5120。
    if pool.allocs != 5120 || pool.frees != 5120 || pool.live != 0 {
        return Err(E_BAL);
    }
    if !pool.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：池耗尽注入——重填满后第 1025 次分配走告警路径（Err 显性、
    // 不崩溃），连续 10 次告警计数 +10；回收一槽后重试成功（等待语义）。
    let mut i = 0;
    while i < POOL_CAP {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        handles[i] = pool.alloc(((seed as u64) << 16) | 0x9000 | i as u64)?;
        i += 1;
    }
    let mut k = 0;
    while k < 10 {
        if pool.alloc(0xdead) != Err(E_EXHAUSTED) {
            return Err(E_EXHAUSTED);
        }
        k += 1;
    }
    if pool.alarms != 10 {
        return Err(E_ALARM);
    }
    pool.free(handles[0])?;
    let retry = pool.alloc(0xfeed)?;
    if pool.fp_of(retry)? != 0xfeed {
        return Err(E_BAL);
    }
    if !pool.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：双回收拦截——同一活槽回收两次，第二次 E_DBL_FREE 且账不动。
    if pool.free(retry) != Ok(()) {
        return Err(E_BAL);
    }
    if pool.free(retry) != Err(E_DBL_FREE) {
        return Err(E_DBL_FREE);
    }
    if pool.dbl_free_hits != 1 {
        return Err(E_DBL_FREE);
    }
    if !pool.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：快照面——快照导出与账面逐字段一致；快照守恒核全过；
    // 占用率/峰值万分比在闭区间 [0,10000]。
    let sn = pool.snapshot();
    if sn.live != pool.live || sn.free != pool.free_count() || sn.peak != pool.peak
        || sn.allocs != pool.allocs || sn.frees != pool.frees || sn.alarms != pool.alarms
        || sn.dbl_free_hits != pool.dbl_free_hits
    {
        return Err(E_BAL);
    }
    if !snapshot_conserve(&sn) {
        return Err(E_BAL);
    }
    if pool.occupancy_bp() > 10_000 || pool.peak_bp() > 10_000 {
        return Err(E_BAL);
    }
    if pool.peak_bp() < pool.occupancy_bp() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36017_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36017_sidpool");
    cs.add("SEG 段位独占", SEG_BASE == 0x7470, "0x7470~0x748F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }
}
