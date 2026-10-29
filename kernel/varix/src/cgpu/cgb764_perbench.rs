#![allow(dead_code)]
//! cgb764_perbench —— CGPU-F0764 感知基准。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0764 感知基准  <SEG 0x6AC0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0764；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌感知面基准台，整数成本模型面）：
//!   1. 基准场景闭集六项（写死语义）：RING_PUSH 环入账 /
//!      MEDIAN 中位排序 / FRAME 帧账 / FNV 指纹 / PACK 打包往返 /
//!      COVER 覆盖桶；
//!   2. 成本模型全部整数（写死单元成本）：环入账 3/次、
//!      中位 n²/4+n、帧账 2/次、指纹 5/字、打包 4/次、
//!      覆盖 n+8；禁浮点、禁计时不敏感面——确定性优先；
//!   3. 预算表写死：每场景（规模, 预算）二元组，超支红码；
//!   4. 结果账：定容 16 槽，守恒恒等式：运行数 = 过关数 + 超支数
//!      = 在库数（无逐出，槽满即拒）；
//!   5. 汇总位型：总成本/最差比/全过关位；最差比 = 千分位
//!      （成本*1000/预算，除零防御）；
//!   6. 确定性复核：同轮双跑结果位型恒同（成本模型纯函数）；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6AC0 段为本模块独占（SEG_BASE=0x6AC0），判别式
//!   SEG_BASE+1..+N；与 0x6AA0/0x6AE0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PB_VERSION: &str = "cgb764_perbench-v1";

/// 诊断段基址（0x6AC0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6AC0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6AC1 场景号越界（0..=5 之外）。
pub const XC_BAD_SCENE: u16 = SEG_BASE + 1;
/// 0x6AC2 规模越界（0 或 >4096）。
pub const XC_BAD_SIZE: u16 = SEG_BASE + 2;
/// 0x6AC3 结果账满（16 槽）。
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 4;
/// 0x6AC4 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 5;
/// 0x6AC5 确定性漂移（双跑位型不一致）。
pub const XC_DETER_DRIFT: u16 = SEG_BASE + 6;
/// 0x6AC6 空账汇总。
pub const XC_EMPTY_SUM: u16 = SEG_BASE + 7;
/// 0x6AC7 预算为 0（除零防御面）。
pub const XC_BAD_BUDGET: u16 = SEG_BASE + 8;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 场景闭集：0=RING_PUSH 1=MEDIAN 2=FRAME 3=FNV 4=PACK 5=COVER。
pub const S_RING: usize = 0;
pub const S_MEDIAN: usize = 1;
pub const S_FRAME: usize = 2;
pub const S_FNV: usize = 3;
pub const S_PACK: usize = 4;
pub const S_COVER: usize = 5;
/// 场景数（写死：6）。
pub const SCENE_CAP: usize = 6;
/// 结果账容量（写死：16）。
pub const LEDGER_CAP: usize = 16;
/// 规模上限（写死：4096）。
pub const SIZE_MAX: u32 = 4096;
/// 单元成本表（写死）：环/帧/指纹/打包按次，覆盖加底 8。
pub const UNIT_RING: u64 = 3;
pub const UNIT_FRAME: u64 = 2;
pub const UNIT_FNV: u64 = 5;
pub const UNIT_PACK: u64 = 4;
pub const UNIT_COVER: u64 = 1;
pub const COVER_BASE: u64 = 8;

/// 场景成本模型（纯函数，整数）。
pub fn cost_of(scene: usize, n: u32) -> Result<u64, u16> {
    if scene >= SCENE_CAP {
        return Err(XC_BAD_SCENE);
    }
    if n == 0 || n > SIZE_MAX {
        return Err(XC_BAD_SIZE);
    }
    let n64 = n as u64;
    Ok(match scene {
        S_RING => n64 * UNIT_RING,
        // 中位：插入排序平均 n²/4 + n。
        S_MEDIAN => (n64 * n64) / 4 + n64,
        S_FRAME => n64 * UNIT_FRAME,
        S_FNV => n64 * UNIT_FNV,
        S_PACK => n64 * UNIT_PACK,
        _ => n64 * UNIT_COVER + COVER_BASE,
    })
}

/// 预算表（场景 → (规模, 预算)）——写死。
pub const BUDGET: [[u32; 2]; SCENE_CAP] = [
    [300, 1000],  // RING: 900 ≤ 1000
    [60, 1100],   // MEDIAN: 960 ≤ 1100
    [400, 900],   // FRAME: 800 ≤ 900
    [200, 1200],  // FNV: 1000 ≤ 1200
    [250, 1100],  // PACK: 1000 ≤ 1100
    [900, 1000],  // COVER: 908 ≤ 1000
];

/// 基准结果条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchRec {
    /// 场景号。
    pub scene: usize,
    /// 规模。
    pub size: u32,
    /// 成本（单元）。
    pub cost: u64,
    /// 预算。
    pub budget: u32,
    /// 过关与否（cost ≤ budget）。
    pub pass: bool,
    /// 千分位比值（cost*1000/budget）。
    pub permil: u32,
}

/// 感知基准台：结果账 + 汇总位型。
/// 守恒恒等式：运行数 = 过关数 + 超支数 = 在库数。
pub struct BenchHarness {
    recs: Vec<BenchRec>,
    passed: u32,
    failed: u32,
}

impl BenchHarness {
    /// 空台。
    pub fn new() -> Self {
        BenchHarness {
            recs: Vec::new(),
            passed: 0,
            failed: 0,
        }
    }

    /// 跑单场景（按预算表行取规模与预算）。
    pub fn run_row(&mut self, scene: usize) -> Result<BenchRec, u16> {
        if scene >= SCENE_CAP {
            return Err(XC_BAD_SCENE);
        }
        let size = BUDGET[scene][0];
        let budget = BUDGET[scene][1];
        self.run(scene, size, budget)
    }

    /// 跑自定义（场景, 规模, 预算）：成本纯函数 + 过关判定 + 入账。
    pub fn run(&mut self, scene: usize, size: u32, budget: u32) -> Result<BenchRec, u16> {
        if scene >= SCENE_CAP {
            return Err(XC_BAD_SCENE);
        }
        if budget == 0 {
            return Err(XC_BAD_BUDGET);
        }
        let cost = cost_of(scene, size)?;
        let pass = cost <= budget as u64;
        let permil = ((cost * 1000) / budget as u64) as u32;
        let rec = BenchRec {
            scene,
            size,
            cost,
            budget,
            pass,
            permil,
        };
        if self.recs.len() >= LEDGER_CAP {
            return Err(XC_LEDGER_FULL);
        }
        self.recs.push(rec);
        if pass {
            self.passed += 1;
        } else {
            self.failed += 1;
        }
        Ok(rec)
    }

    /// 全预算表六场景连跑。
    pub fn run_all(&mut self) -> Result<(), u16> {
        for s in 0..SCENE_CAP {
            self.run_row(s)?;
        }
        Ok(())
    }

    /// 汇总位型：总成本(0..16 位) | 全过关位(1<<16) | 运行数(24..32)。
    pub fn summary(&self) -> Result<u64, u16> {
        if self.recs.is_empty() {
            return Err(XC_EMPTY_SUM);
        }
        let mut total: u64 = 0;
        for r in self.recs.iter() {
            total += r.cost;
        }
        let all_pass = if self.failed == 0 { 1u64 << 16 } else { 0 };
        let runs = (self.recs.len() as u64) << 24;
        Ok((total & 0xFFFF) | all_pass | runs)
    }

    /// 最差千分位比值。
    pub fn worst_permil(&self) -> Result<u32, u16> {
        if self.recs.is_empty() {
            return Err(XC_EMPTY_SUM);
        }
        let mut w = 0u32;
        for r in self.recs.iter() {
            if r.permil > w {
                w = r.permil;
            }
        }
        Ok(w)
    }

    /// 守恒核：运行数 = 过关 + 超支 = 在库。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.passed + self.failed != self.recs.len() as u32 {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 在库数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 确定性复核：重建空台同批连跑，结果逐条一致。
    pub fn determin_check(&self) -> Result<(), u16> {
        let mut h2 = BenchHarness::new();
        for r in self.recs.iter() {
            let r2 = h2.run(r.scene, r.size, r.budget)?;
            if r2 != *r {
                return Err(XC_DETER_DRIFT);
            }
        }
        Ok(())
    }
}

/// 自例程：全场景跑批 → 预算过关 → 汇总位型 → 最差比 → 守恒 →
/// 确定性 → 错误路径（坏场景/坏规模/坏预算/空汇总）。
pub fn drill() -> Result<u32, u16> {
    // 成本模型直查。
    if cost_of(S_RING, 300)? != 900 {
        return Err(XC_BAD_SCENE);
    }
    if cost_of(S_MEDIAN, 60)? != 960 {
        return Err(XC_BAD_SCENE);
    }
    if cost_of(S_FRAME, 400)? != 800 {
        return Err(XC_BAD_SCENE);
    }
    if cost_of(S_FNV, 200)? != 1000 {
        return Err(XC_BAD_SCENE);
    }
    if cost_of(S_PACK, 250)? != 1000 {
        return Err(XC_BAD_SCENE);
    }
    if cost_of(S_COVER, 900)? != 908 {
        return Err(XC_BAD_SCENE);
    }
    // 错误路径 1：坏场景/坏规模。
    if cost_of(6, 10) != Err(XC_BAD_SCENE) {
        return Err(XC_BAD_SCENE);
    }
    if cost_of(S_RING, 0) != Err(XC_BAD_SIZE) {
        return Err(XC_BAD_SIZE);
    }
    if cost_of(S_RING, SIZE_MAX + 1) != Err(XC_BAD_SIZE) {
        return Err(XC_BAD_SIZE);
    }
    // 主路径：全预算表六场景。
    let mut h = BenchHarness::new();
    h.run_all()?;
    h.conserved()?;
    if h.len() != SCENE_CAP {
        return Err(XC_UNBALANCED);
    }
    // 全过关（预算表写死保证）。
    let s = h.summary()?;
    if s & (1u64 << 16) == 0 {
        return Err(XC_UNBALANCED);
    }
    // 总成本位段：5568（900+960+800+1000+1000+908）。
    if s & 0xFFFF != 5568 {
        return Err(XC_UNBALANCED);
    }
    // 运行数位段 = 6。
    if (s >> 24) & 0xFF != 6 {
        return Err(XC_UNBALANCED);
    }
    // 最差比：中位 960/1100 = 872（取整）… 实为 872.72 → 872。
    let w = h.worst_permil()?;
    if w < 800 || w > 1000 {
        return Err(XC_UNBALANCED);
    }
    // 确定性复核。
    h.determin_check()?;
    // 超支路径：小预算强制超支（中位 60 规模 960 > 500）。
    let mut h2 = BenchHarness::new();
    let r2 = h2.run(S_MEDIAN, 60, 500)?;
    if r2.pass {
        return Err(XC_UNBALANCED);
    }
    if r2.permil != 1920 {
        return Err(XC_UNBALANCED);
    }
    h2.conserved()?;
    let s2 = h2.summary()?;
    if s2 & (1u64 << 16) != 0 {
        return Err(XC_UNBALANCED);
    }
    // 错误路径 2：坏预算 / 空汇总 / 账满。
    let mut h3 = BenchHarness::new();
    if h3.run(S_RING, 10, 0) != Err(XC_BAD_BUDGET) {
        return Err(XC_BAD_BUDGET);
    }
    if h3.summary() != Err(XC_EMPTY_SUM) {
        return Err(XC_EMPTY_SUM);
    }
    if h3.worst_permil() != Err(XC_EMPTY_SUM) {
        return Err(XC_EMPTY_SUM);
    }
    let mut h4 = BenchHarness::new();
    for i in 0..LEDGER_CAP {
        h4.run(S_RING, 10 + i as u32, 10_000)?;
    }
    if h4.run(S_RING, 10, 10_000) != Err(XC_LEDGER_FULL) {
        return Err(XC_LEDGER_FULL);
    }
    h4.conserved()?;
    Ok(0)
}
