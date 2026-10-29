#![allow(dead_code)]
//! cgb765_perconsist —— CGPU-F0765 感知一致性。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0765 感知一致性  <SEG 0x6AE0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0765；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知面双径一致性核查，整数面）：
//!   1. 双径同题：A 径 = 增量维护（样本环 + 滚动和，逐条 O(1)），
//!      B 径 = 批量重算（同环全扫，逐查 O(n)）；同一 16 槽窗口环
//!      为唯一事实源，两径必须同值；
//!   2. 四字段闭集比对：sum/cnt/min/max，逐检查点全比；
//!   3. 检查点账：每推一样即查一次，守恒恒等式：
//!      检查数 = 通过数 + 失配数；失配账定容 16 槽（满则逐出最老），
//!      失配账 = 在库 + 逐出；
//!   4. 故障注入：可对 A 径和值注入偏移（确定性），注入后下一
//!      检查点必须失配于 sum 字段——验证核查器自身能抓错；
//!   5. 确定性复核：同序列双跑 A 径聚合恒同（漂移红码）；
//!   6. 样本合法域 1..=100000；窗口环守恒：入账数 = 在环 + 逐出；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6AE0 段为本模块独占（SEG_BASE=0x6AE0），判别式
//!   SEG_BASE+1..+N；与 0x6AC0/0x6B00 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PC_VERSION: &str = "cgb765_perconsist-v1";

/// 诊断段基址（0x6AE0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6AE0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6AE1 样本非法（0 或 >100000）。
pub const XC_BAD_SAMPLE: u16 = SEG_BASE + 1;
/// 0x6AE2 窗口环账不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 2;
/// 0x6AE3 sum 字段失配。
pub const XC_MM_SUM: u16 = SEG_BASE + 3;
/// 0x6AE4 cnt 字段失配。
pub const XC_MM_CNT: u16 = SEG_BASE + 4;
/// 0x6AE5 min 字段失配。
pub const XC_MM_MIN: u16 = SEG_BASE + 5;
/// 0x6AE6 max 字段失配。
pub const XC_MM_MAX: u16 = SEG_BASE + 6;
/// 0x6AE7 零检查查询。
pub const XC_NO_CHECKS: u16 = SEG_BASE + 7;
/// 0x6AE8 确定性漂移。
pub const XC_DETER_DRIFT: u16 = SEG_BASE + 8;
/// 0x6AE9 失配账越界（字段号 > 4）。
pub const XC_BAD_FIELD: u16 = SEG_BASE + 9;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 窗口环容量（写死：16）。
pub const RING_CAP: usize = 16;
/// 样本上限（写死：100000）。
pub const SAMPLE_MAX: u32 = 100_000;
/// 失配账容量（写死：16）。
pub const MM_CAP: usize = 16;
/// 比对字段闭集：1=sum 2=cnt 3=min 4=max。
pub const F_SUM: u8 = 1;
pub const F_CNT: u8 = 2;
pub const F_MIN: u8 = 3;
pub const F_MAX: u8 = 4;

/// 四元聚合（整数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Agg {
    pub sum: u64,
    pub cnt: u32,
    pub min: u32,
    pub max: u32,
}

/// 16 槽样本窗口环（双径共用事实源）。
pub struct WindowRing {
    buf: [Option<u32>; RING_CAP],
    head: usize,
    in_ring: usize,
    evicted: usize,
    pushed: usize,
}

impl WindowRing {
    /// 空环。
    pub fn new() -> Self {
        WindowRing {
            buf: [None; RING_CAP],
            head: 0,
            in_ring: 0,
            evicted: 0,
            pushed: 0,
        }
    }

    /// 样本合法性。
    pub fn legal(v: u32) -> Result<(), u16> {
        if v == 0 || v > SAMPLE_MAX {
            return Err(XC_BAD_SAMPLE);
        }
        Ok(())
    }

    /// 入账，满则逐出最老并返回被逐值。
    pub fn push(&mut self, v: u32) -> Result<Option<u32>, u16> {
        Self::legal(v)?;
        let evicted = if self.in_ring < RING_CAP {
            self.in_ring += 1;
            None
        } else {
            let old = self.buf[self.head];
            self.evicted += 1;
            old
        };
        self.buf[self.head] = Some(v);
        self.head = (self.head + 1) % RING_CAP;
        self.pushed += 1;
        Ok(evicted)
    }

    /// 全扫聚合（B 径基元，也是 min/max 提取基元）。
    pub fn scan(&self) -> Agg {
        let mut a = Agg {
            sum: 0,
            cnt: 0,
            min: 0,
            max: 0,
        };
        let mut first = true;
        for slot in self.buf.iter().flatten() {
            let v = *slot;
            a.sum += v as u64;
            a.cnt += 1;
            if first || v < a.min {
                a.min = v;
            }
            if first || v > a.max {
                a.max = v;
            }
            first = false;
        }
        a
    }

    /// 在环数。
    pub fn count(&self) -> usize {
        self.in_ring
    }

    /// 守恒：入账数 = 在环 + 逐出。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.pushed != self.in_ring + self.evicted {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }
}

/// A 径：增量维护和值（逐条 O(1)），min/max 取自全扫。
pub struct PathA {
    ring: WindowRing,
    run_sum: u64,
}

impl PathA {
    /// 空径。
    pub fn new() -> Self {
        PathA {
            ring: WindowRing::new(),
            run_sum: 0,
        }
    }

    /// 增量入账：滚动和同步增减。
    pub fn push(&mut self, v: u32) -> Result<(), u16> {
        if let Some(old) = self.ring.push(v)? {
            self.run_sum -= old as u64;
        }
        self.run_sum += v as u64;
        Ok(())
    }

    /// 聚合视图：增量 sum + 全扫 cnt/min/max。
    pub fn agg(&self) -> Agg {
        let mut a = self.ring.scan();
        a.sum = self.run_sum;
        a
    }

    /// 守恒。
    pub fn conserved(&self) -> Result<(), u16> {
        self.ring.conserved()
    }
}

/// 失配账条目（检查点号 + 字段号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MmRec {
    pub checkpoint: u32,
    pub field: u8,
}

/// 双径一致性核查器。
/// 守恒恒等式：检查数 = 通过数 + 失配数；失配账 = 在库 + 逐出。
pub struct ConsistChecker {
    a: PathA,
    b_ring: WindowRing,
    fault: i64,
    checks: u32,
    passed: u32,
    mismatched: u32,
    ledger: Vec<MmRec>,
    ledger_evicted: u32,
    checkpoint: u32,
}

impl ConsistChecker {
    /// 空核查器（A/B 各持独立环副本，同序喂入）。
    pub fn new() -> Self {
        ConsistChecker {
            a: PathA::new(),
            b_ring: WindowRing::new(),
            fault: 0,
            checks: 0,
            passed: 0,
            mismatched: 0,
            ledger: Vec::new(),
            ledger_evicted: 0,
            checkpoint: 0,
        }
    }

    /// 故障注入：对 A 径和值报告面加偏移。
    pub fn inject(&mut self, delta: i64) {
        self.fault = delta;
    }

    /// 双径同步入账 + 检查点比对。
    pub fn feed(&mut self, v: u32) -> Result<(), u16> {
        self.a.push(v)?;
        self.b_ring.push(v)?;
        self.checkpoint += 1;
        self.checks += 1;
        let av = self.a.agg();
        let bv = self.b_ring.scan();
        // 故障面：A 径和值加偏移后为负则截 0。
        let mut a_sum = av.sum as i64 + self.fault;
        if a_sum < 0 {
            a_sum = 0;
        }
        let a_view = Agg {
            sum: a_sum as u64,
            cnt: av.cnt,
            min: av.min,
            max: av.max,
        };
        if a_view == bv {
            self.passed += 1;
            return Ok(());
        }
        // 逐字段定位失配（sum → cnt → min → max）。
        let field = if a_view.sum != bv.sum {
            XC_MM_SUM
        } else if a_view.cnt != bv.cnt {
            XC_MM_CNT
        } else if a_view.min != bv.min {
            XC_MM_MIN
        } else {
            XC_MM_MAX
        };
        self.mismatched += 1;
        if self.ledger.len() >= MM_CAP {
            self.ledger.remove(0);
            self.ledger_evicted += 1;
        }
        self.ledger.push(MmRec {
            checkpoint: self.checkpoint,
            field,
        });
        Err(field)
    }

    /// 批量喂入（遇失配继续，账满逐出）。
    pub fn feed_all(&mut self, vs: &[u32]) {
        for v in vs.iter() {
            let _ = self.feed(*v);
        }
    }

    /// 检查统计。
    pub fn stats(&self) -> Result<(u32, u32, u32), u16> {
        if self.checks == 0 {
            return Err(XC_NO_CHECKS);
        }
        Ok((self.checks, self.passed, self.mismatched))
    }

    /// 失配账在库数。
    pub fn mm_len(&self) -> usize {
        self.ledger.len()
    }

    /// 守恒核：检查数 = 通过 + 失配；失配账 = 在库 + 逐出；
    /// 双环各自守恒且同步（在环数恒等）。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.checks != self.passed + self.mismatched {
            return Err(XC_UNBALANCED);
        }
        if (self.ledger.len() as u32) + self.ledger_evicted != self.mismatched {
            return Err(XC_UNBALANCED);
        }
        self.a.conserved()?;
        self.b_ring.conserved()?;
        if self.a.ring.count() != self.b_ring.count() {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }
}

/// 自例程：双径同喂 → 全查点通过 → 故障注入抓 sum 失配 →
/// 失配账滚动 → 确定性复核 → 守恒 → 错误路径（坏样本/零检查）。
pub fn drill() -> Result<u32, u16> {
    // 主路径：20 样本（>16 触发逐出），逐检查点全过。
    let mut c = ConsistChecker::new();
    for i in 1..=20u32 {
        c.feed(i * 100)?;
    }
    let (checks, passed, mm) = c.stats()?;
    if checks != 20 || passed != 20 || mm != 0 {
        return Err(XC_UNBALANCED);
    }
    c.conserved()?;
    if c.mm_len() != 0 {
        return Err(XC_UNBALANCED);
    }
    // 故障注入：A 径和值 +7 → 下一点必失配 sum。
    c.inject(7);
    if c.feed(2100) != Err(XC_MM_SUM) {
        return Err(XC_MM_SUM);
    }
    if c.mm_len() != 1 {
        return Err(XC_UNBALANCED);
    }
    // 撤销注入 → 恢复通过。
    c.inject(0);
    c.feed(2200)?;
    let (c2, p2, m2) = c.stats()?;
    if c2 != 22 || p2 != 21 || m2 != 1 {
        return Err(XC_UNBALANCED);
    }
    // 失配账滚动：连续 20 次注入失配（>16 → 逐出 5）。
    let mut c3 = ConsistChecker::new();
    c3.feed(100)?;
    c3.inject(1);
    for _ in 0..20u32 {
        let _ = c3.feed(100);
    }
    c3.conserved()?;
    if c3.mm_len() != MM_CAP {
        return Err(XC_UNBALANCED);
    }
    // 负偏移注入：和值截 0 面 → 仍失配 sum。
    let mut c4 = ConsistChecker::new();
    c4.feed(500)?;
    c4.inject(-1000);
    if c4.feed(500) != Err(XC_MM_SUM) {
        return Err(XC_MM_SUM);
    }
    // 确定性复核：同序列双 A 径聚合恒同（含逐出）。
    let mut a1 = PathA::new();
    let mut a2 = PathA::new();
    for i in 1..=40u32 {
        a1.push(i * 7)?;
        a2.push(i * 7)?;
    }
    if a1.agg() != a2.agg() {
        return Err(XC_DETER_DRIFT);
    }
    // 40 样本 → 逐出 24，在环 16；和值 = 保留窗内 25..40 的 7 倍。
    let g = a1.agg();
    if g.cnt != 16 {
        return Err(XC_UNBALANCED);
    }
    let mut want: u64 = 0;
    for i in 25..=40u32 {
        want += (i * 7) as u64;
    }
    if g.sum != want {
        return Err(XC_UNBALANCED);
    }
    if g.min != 25 * 7 || g.max != 40 * 7 {
        return Err(XC_MM_MIN);
    }
    // 错误路径 1：坏样本（0 / 越上限）。
    let mut c5 = ConsistChecker::new();
    if c5.feed(0) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    if c5.feed(SAMPLE_MAX + 1) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    // 错误路径 2：零检查统计。
    if c5.stats() != Err(XC_NO_CHECKS) {
        return Err(XC_NO_CHECKS);
    }
    Ok(0)
}
