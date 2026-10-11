//! cgd13_timebench — CGPU-F0494 帧计时基准（计时器自己花掉的时间也要记账）。
//!
//! 判据原文（施工书锚点 CGPU-F0494）：基准：计时开销基准（<0.1% 实测）；
//! 账本写入吞吐；查询延迟——入册回归。判据：三基准、入册、判据。
//!
//! 职责定位：三基准框架（计时开销比/账本写入吞吐/查询延迟——口径固定、
//! 结果入册可回归对拍）+ 开销比模型（计时器操作步数 vs 帧预算步数——<
//! 0.1% 红线的确定性度量：no_std 无墙钟，用操作步数当时间轴，与 cgc04
//! 50µs 步数化同范式）+ 吞吐基准（批量写入的摊销步数/条）+ 查询延迟基准
//! （单查询最坏步数——线性扫描红线化）+ 入册回归（基准结果带指纹写入基
//! 准册，下轮跑同口径对拍——漂移即回归立案）。红线：基准不可作弊（固定
//! 载荷+固定迭代数+固定口径——参数全部显性入册，不跑"挑好看的参数"）。
//! 诊断码段 0x5D01~0x5D05。

use alloc::vec::Vec;

/// 模块版本。
pub const TIME_BENCH_VERSION: &str = "D13-timebench-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x5D 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_OVERHEAD_EXCEED: u32 = 0x5D01; // 计时开销超 0.1% 红线
pub const XC_THROUGHPUT_LOW: u32 = 0x5D02; // 写入吞吐低于红线（摊销超标）
pub const XC_QUERY_SLOW: u32 = 0x5D03; // 查询延迟超红线
pub const XC_REGRESSION_DRIFT: u32 = 0x5D04; // 入册回归漂移（同口径结果变了）
pub const XC_BENCH_PARAM_DIRTY: u32 = 0x5D05; // 基准参数未入册（口径不显性）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 计时开销红线（相对帧预算）：0.1% = 1000 ppm。
pub const OVERHEAD_REDLINE_PPM: u64 = 1_000;
/// 帧预算步数（12.5ms 帧在 100 步/µs 口径下的代理量——确定性时间轴）。
pub const FRAME_BUDGET_STEPS: u64 = 1_250_000;
/// 写入吞吐红线：单条摊销 ≤ 40 步。
pub const WRITE_AMORT_REDLINE: u64 = 40;
/// 查询延迟红线：单查询 ≤ 96 步（约 600 条目线性扫描的常数上界）。
pub const QUERY_STEPS_REDLINE: u64 = 96;
/// 基准册容量。
pub const BENCH_LEDGER_CAP: usize = 32;

/// 计时开销模型（被试计时器的操作步数账）。
pub struct OverheadModel {
    /// 每次 mark 操作的步数（被试方注入的实测代理）。
    pub mark_steps: u64,
    /// 每次 start 操作的步数。
    pub start_steps: u64,
    /// 每帧 mark 次数（四段口径——cgd02 的段数）。
    pub marks_per_frame: u64,
}

impl OverheadModel {
    /// 单帧计时开销总步数。
    pub const fn frame_overhead_steps(&self) -> u64 {
        self.start_steps + self.mark_steps * self.marks_per_frame
    }

    /// 开销比（ppm）：开销步数 / 帧预算步数 × 1e6。
    pub const fn overhead_ppm(&self) -> u64 {
        self.frame_overhead_steps() * 1_000_000 / FRAME_BUDGET_STEPS
    }

    /// 红线核验（判据"<0.1% 实测"）。
    pub const fn check(&self) -> Result<(), u32> {
        if self.overhead_ppm() >= OVERHEAD_REDLINE_PPM {
            return Err(XC_OVERHEAD_EXCEED);
        }
        Ok(())
    }
}

/// 有界账本（被试对象：写入吞吐与查询延迟的测量靶）。
pub struct BenchLedger {
    slots: Vec<(u64, u64)>,
    cap: usize,
    /// 操作步数账（写入/查询分别累计）。
    pub write_steps: u64,
    pub query_steps: u64,
    pub writes: u64,
    pub queries: u64,
}

impl BenchLedger {
    pub fn new(cap: usize) -> Self {
        BenchLedger {
            slots: Vec::with_capacity(cap),
            cap,
            write_steps: 0,
            query_steps: 0,
            writes: 0,
            queries: 0,
        }
    }

    /// 写入一条（重复键覆盖；步数模型：追加 3 步/覆盖 2 步+扫描）。
    pub fn put(&mut self, key: u64, val: u64) {
        self.write_steps += 3; // 定界记账
        let mut i = 0;
        while i < self.slots.len() {
            self.write_steps += 1;
            if self.slots[i].0 == key {
                self.slots[i].1 = val;
                return;
            }
            i += 1;
        }
        if self.slots.len() < self.cap {
            self.slots.push((key, val));
        }
        self.writes += 1;
    }

    /// 查询（步数模型：逐位比较 1 步/项）。
    pub fn get(&mut self, key: u64) -> Option<u64> {
        self.query_steps += 1;
        let mut i = 0;
        while i < self.slots.len() {
            self.query_steps += 1;
            if self.slots[i].0 == key {
                self.queries += 1;
                return Some(self.slots[i].1);
            }
            i += 1;
        }
        self.queries += 1;
        None
    }

    /// 写入摊销（步/条）。
    pub fn write_amortized(&self) -> u64 {
        if self.writes == 0 {
            return u64::MAX;
        }
        self.write_steps / self.writes
    }

    /// 查询摊销（步/次）。
    pub fn query_amortized(&self) -> u64 {
        if self.queries == 0 {
            return u64::MAX;
        }
        self.query_steps / self.queries
    }
}

/// 基准册条目（入册回归的账面）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BenchRecord {
    pub name_id: u64,
    /// 结果值（开销 ppm / 摊销步数 / 查询步数——按 kind 解释）。
    pub value: u64,
    /// 口径指纹（参数集 FNV——判据"参数全部显性入册"）。
    pub param_sig: u64,
}

/// 基准编排器（三基准+入册回归）。
pub struct TimeBench {
    ledger: Vec<BenchRecord>,
    cap: usize,
    pub runs: u64,
    pub regressions: u64,
}

impl TimeBench {
    pub fn new() -> Self {
        TimeBench {
            ledger: Vec::with_capacity(BENCH_LEDGER_CAP),
            cap: BENCH_LEDGER_CAP,
            runs: 0,
            regressions: 0,
        }
    }

    /// 口径指纹（参数集 FNV——防"挑参数"作弊：同 name_id 必须同口径对拍）。
    pub const fn param_sig(iterations: u64, payload: u64, kind: u8) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_step_b(h, iterations);
        h = fnv_step_b(h, payload);
        h = fnv_step_b(h, kind as u64);
        h
    }

    /// 基准一：计时开销比（判据"<0.1% 实测"）。
    pub fn bench_overhead(&mut self, model: &OverheadModel, iterations: u64) -> Result<u64, u32> {
        if iterations == 0 {
            return Err(XC_BENCH_PARAM_DIRTY);
        }
        self.runs += 1;
        let mut worst = 0u64;
        let mut i = 0;
        while i < iterations {
            let ppm = model.overhead_ppm();
            if ppm > worst {
                worst = ppm;
            }
            model.check()?;
            i += 1;
        }
        self.record(1, worst, Self::param_sig(iterations, model.marks_per_frame, 1))?;
        Ok(worst)
    }

    /// 基准二：账本写入吞吐（判据"账本写入吞吐"）。
    pub fn bench_write(&mut self, entries: u64) -> Result<u64, u32> {
        if entries == 0 {
            return Err(XC_BENCH_PARAM_DIRTY);
        }
        self.runs += 1;
        let mut l = BenchLedger::new(entries as usize);
        let mut i = 0;
        while i < entries {
            l.put(i, i * 7);
            i += 1;
        }
        let amort = l.write_amortized();
        if amort > WRITE_AMORT_REDLINE {
            return Err(XC_THROUGHPUT_LOW);
        }
        self.record(2, amort, Self::param_sig(entries, 0, 2))?;
        Ok(amort)
    }

    /// 基准三：查询延迟（判据"查询延迟"——满账本最坏查询）。
    pub fn bench_query(&mut self, entries: u64) -> Result<u64, u32> {
        if entries == 0 {
            return Err(XC_BENCH_PARAM_DIRTY);
        }
        self.runs += 1;
        let mut l = BenchLedger::new(entries as usize);
        let mut i = 0;
        while i < entries {
            l.put(i, i * 7);
            i += 1;
        }
        // 查最坏键（不存在——全表扫描）+ 一半存在的。
        let mut j = 0;
        while j < entries {
            let _ = l.get(if j % 2 == 0 { j } else { entries + j });
            j += 1;
        }
        let amort = l.query_amortized();
        if amort > QUERY_STEPS_REDLINE {
            return Err(XC_QUERY_SLOW);
        }
        self.record(3, amort, Self::param_sig(entries, 0, 3))?;
        Ok(amort)
    }

    /// 入册（判据"入册回归"：同口径已有记录 → 对拍漂移；新口径 → 追加）。
    fn record(&mut self, name_id: u64, value: u64, param_sig: u64) -> Result<(), u32> {
        let mut i = 0;
        while i < self.ledger.len() {
            if self.ledger[i].name_id == name_id {
                if self.ledger[i].param_sig == param_sig {
                    // 同口径：值必须一致（确定性基准——漂移即回归）。
                    if self.ledger[i].value != value {
                        self.regressions += 1;
                        return Err(XC_REGRESSION_DRIFT);
                    }
                    return Ok(());
                }
                // 同名不同口径：更新口径（参数显性变化是合法演进）。
                self.ledger[i] = BenchRecord {
                    name_id,
                    value,
                    param_sig,
                };
                return Ok(());
            }
            i += 1;
        }
        if self.ledger.len() >= self.cap {
            self.ledger.remove(0);
        }
        self.ledger.push(BenchRecord {
            name_id,
            value,
            param_sig,
        });
        Ok(())
    }

    /// 册查询。
    pub fn lookup(&self, name_id: u64) -> Option<&BenchRecord> {
        let mut i = 0;
        while i < self.ledger.len() {
            if self.ledger[i].name_id == name_id {
                return Some(&self.ledger[i]);
            }
            i += 1;
        }
        None
    }

    /// 册规模。
    pub const fn record_count(&self) -> usize {
        self.ledger.len()
    }
}

pub const fn fnv_step_b(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut i = 0;
    // 8 字节逐位混合（const fn 循环——while 索引）。
    while i < 8 {
        x = (x ^ ((v >> (i * 8)) & 0xFF)).wrapping_mul(0x1000_0000_01b3);
        i += 1;
    }
    x
}

// ---------------------------------------------------------------------------
// 自检（判据四组：开销比 / 吞吐 / 查询延迟 / 入册回归）
// ---------------------------------------------------------------------------

impl TimeBench {
    /// 组一：开销比——达标模型过、超标模型拒。
    pub fn selfcheck_overhead(&mut self) -> Result<(), u32> {
        let good = OverheadModel {
            start_steps: 8,
            mark_steps: 6,
            marks_per_frame: 5,
        };
        // 8+30=38 步 → 38e6/1.25e6=30 ppm < 1000。
        let mut b = TimeBench::new();
        let ppm = b.bench_overhead(&good, 3)?;
        if ppm != 30 {
            return Err(XC_OVERHEAD_EXCEED);
        }
        let bad = OverheadModel {
            start_steps: 800,
            mark_steps: 600,
            marks_per_frame: 5,
        };
        // 3800 步 → 3040 ppm 超红线。
        let mut b2 = TimeBench::new();
        if b2.bench_overhead(&bad, 1) != Err(XC_OVERHEAD_EXCEED) {
            return Err(XC_OVERHEAD_EXCEED);
        }
        // 零迭代参数脏。
        if b.bench_overhead(&good, 0) != Err(XC_BENCH_PARAM_DIRTY) {
            return Err(XC_BENCH_PARAM_DIRTY);
        }
        Ok(())
    }

    /// 组二：吞吐——线性扫描模型的摊销随规模超红线（大账本拒、小账本过）。
    pub fn selfcheck_write(&mut self) -> Result<(), u32> {
        let mut b = TimeBench::new();
        // 256 条：摊销=3+255/2≈130 步 > 40 红线 → 期望拒（红线语义验证）。
        if b.bench_write(256) != Err(XC_THROUGHPUT_LOW) {
            return Err(XC_THROUGHPUT_LOW);
        }
        Ok(())
    }

    /// 组二重写：用容量等长账本验证摊销口径与红线联动。
    pub fn selfcheck_write2(&mut self) -> Result<(), u32> {
        // 小账本（16 条）摊销=3+15/2≈10 < 40 → 过。
        let mut b = TimeBench::new();
        let amort = b.bench_write(16)?;
        if amort == 0 || amort >= WRITE_AMORT_REDLINE {
            return Err(XC_THROUGHPUT_LOW);
        }
        if b.lookup(2).is_none() {
            return Err(XC_BENCH_PARAM_DIRTY); // 入册缺失
        }
        Ok(())
    }

    /// 组三：查询延迟——半命中半未命中摊销在红线内、大表拒。
    pub fn selfcheck_query(&mut self) -> Result<(), u32> {
        let mut b = TimeBench::new();
        // 16 条账本：查询摊销≈1+8=9 步 < 96 → 过。
        let amort = b.bench_query(16)?;
        if amort == 0 || amort >= QUERY_STEPS_REDLINE {
            return Err(XC_QUERY_SLOW);
        }
        // 大表（1024）：未命中扫描 1024 → 摊销≈512 > 96 → 拒。
        let mut b2 = TimeBench::new();
        if b2.bench_query(1024) != Err(XC_QUERY_SLOW) {
            return Err(XC_QUERY_SLOW);
        }
        Ok(())
    }

    /// 组四：入册回归——同口径复跑一致、同口径值漂移立案、口径变更合法演进。
    pub fn selfcheck_regression(&mut self) -> Result<(), u32> {
        let mut b = TimeBench::new();
        let good = OverheadModel {
            start_steps: 8,
            mark_steps: 6,
            marks_per_frame: 5,
        };
        let p1 = b.bench_overhead(&good, 3)?;
        let p2 = b.bench_overhead(&good, 3)?; // 同口径复跑
        if p1 != p2 {
            return Err(XC_REGRESSION_DRIFT);
        }
        if b.record_count() != 1 {
            return Err(XC_BENCH_PARAM_DIRTY);
        }
        // 同名不同口径：marks_per_frame 变 → 参数指纹变 → 合法演进更新。
        let alt = OverheadModel {
            start_steps: 8,
            mark_steps: 6,
            marks_per_frame: 8,
        };
        let _ = b.bench_overhead(&alt, 3)?;
        if b.record_count() != 1 {
            return Err(XC_BENCH_PARAM_DIRTY); // 仍是一条（更新非追加）
        }
        // 直接构造漂移：手工入册后改值。
        let mut b3 = TimeBench::new();
        b3.record(9, 100, 777)?;
        if b3.record(9, 100, 777).is_err() {
            return Err(XC_REGRESSION_DRIFT);
        }
        if b3.record(9, 200, 777) != Err(XC_REGRESSION_DRIFT) || b3.regressions != 1 {
            return Err(XC_REGRESSION_DRIFT);
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_overhead()?;
        self.selfcheck_write()?;
        self.selfcheck_write2()?;
        self.selfcheck_query()?;
        self.selfcheck_regression()?;
        Ok(())
    }
}
