//! cgb860_amdbench —— CGPU-F0860 AMD 基准。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0860 AMD 基准  <SEG 0x7600>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0860；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（AMD 基准，整数分面）：
//!   1. 基准项闭集八项（写死）：光栅吞吐 / 异步计算 / 原子操作 /
//!      IC 命中带 / VRAM 带宽 / 几何吞吐 / 网格着色 / 光追吞吐；
//!      项号 0..=7，越界红码（XC_BAD_ITEM）；
//!   2. 每项定容分数环（8 槽），分数千分位 0..=100000；环账守恒：
//!      入账 = 在环 + 逐出；
//!   3. 代际修正系数（写死）：GCN4 ×80 / GCN5 ×90 / RDNA1 ×100 /
//!      RDNA2 ×110 / RDNA3 ×125（千分位系数），修正后分数回超上
//!      限则截断到上限；
//!   4. 总分账守恒（恒等式）：总分 = Σ(项均值 × 项权) / 权和；权
//!      表和恒 1000，漂移红码（XC_WEIGHT_DRIFT）；
//!   5. 代际闭集五档，越界红码（XC_BAD_GEN）；
//!   6. 跑批状态机：Armed → Running → Report；Report 只读；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x7600 段为本模块独占（SEG_BASE=0x7600），判别式
//!   SEG_BASE+1..+N；与 0x75E0/0x7620 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

/// 模块版本。
pub const AB_VERSION: &str = "cgb860_amdbench-v1";

/// 诊断段基址（0x7600 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7600;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x7601 基准项越界（0..=7 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x7602 分数越界（0..=100000）。
pub const XC_BAD_SCORE: u16 = SEG_BASE + 2;
/// 0x7603 分数环账漂移。
pub const XC_RING_DRIFT: u16 = SEG_BASE + 3;
/// 0x7604 权重账漂移（权和 ≠ 1000）。
pub const XC_WEIGHT_DRIFT: u16 = SEG_BASE + 4;
/// 0x7605 代际越界。
pub const XC_BAD_GEN: u16 = SEG_BASE + 5;
/// 0x7606 状态机违例（未 Arm 即 Run / Report 期写入）。
pub const XC_BAD_PHASE: u16 = SEG_BASE + 6;
/// 0x7607 空项出报告。
pub const XC_EMPTY_ITEM: u16 = SEG_BASE + 7;
/// 0x7608 跑批号倒挂。
pub const XC_RUN_BACKWARD: u16 = SEG_BASE + 8;

/// 基准项数（写死：8）。
pub const ITEM_CAP: usize = 8;
/// 分数环容量（写死：8）。
pub const RING_CAP: usize = 8;
/// 分数上限（千分位）。
pub const SCORE_MAX: u32 = 100000;
/// 权和（千分位）。
pub const WEIGHT_SUM: u32 = 1000;
/// 代际数（写死：5）。
pub const GEN_CAP: usize = 5;

/// 基准项闭集。
pub const I_RASTER: usize = 0;
pub const I_ASYNC: usize = 1;
pub const I_ATOMIC: usize = 2;
pub const I_ICBW: usize = 3;
pub const I_VRAMBW: usize = 4;
pub const I_GEO: usize = 5;
pub const I_MESH: usize = 6;
pub const I_RAYTRACE: usize = 7;

/// 八项权重表（和恒 1000）。
pub const WEIGHTS: [u32; ITEM_CAP] = [160, 150, 110, 120, 150, 110, 100, 100];

/// 代际修正系数（千分位）。
pub const GEN_FACTOR: [u32; GEN_CAP] = [80, 90, 100, 110, 125];

/// 代际名。
pub fn gen_name(g: usize) -> &'static str {
    match g {
        0 => "GCN4",
        1 => "GCN5",
        2 => "RDNA1",
        3 => "RDNA2",
        _ => "RDNA3",
    }
}

/// 跑批状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BPhase {
    /// 备跑。
    Armed,
    /// 跑批中。
    Running,
    /// 出报告。
    Report,
}

/// 项分数环。
pub struct ItemRing {
    /// 在环分数。
    pub ring: Vec<u32>,
    /// 环头。
    pub head: usize,
    /// 入账数。
    pub pushed: u64,
    /// 逐出数。
    pub evicted: u64,
    /// 最高分。
    pub best: u32,
}

impl ItemRing {
    fn new() -> Self {
        ItemRing {
            ring: Vec::new(),
            head: 0,
            pushed: 0,
            evicted: 0,
            best: 0,
        }
    }

    /// 守恒。
    pub fn balanced(&self) -> bool {
        self.pushed == self.ring.len() as u64 + self.evicted
    }

    /// 均值。
    pub fn mean(&self) -> u32 {
        if self.ring.is_empty() {
            return 0;
        }
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < self.ring.len() {
            acc += self.ring[i] as u64;
            i += 1;
        }
        (acc / self.ring.len() as u64) as u32
    }
}

/// AMD 基准册。
pub struct AmdBench {
    /// 代际号。
    pub gen: usize,
    /// 跑批状态。
    pub phase: BPhase,
    /// 八项环。
    pub items: [ItemRing; ITEM_CAP],
    /// 各项末跑批号。
    pub last_run: [u32; ITEM_CAP],
    /// 累计红码。
    pub red_count: u64,
}

impl AmdBench {
    /// 备跑（代际闭集校验）。
    pub fn arm(gen: usize) -> Result<Self, u16> {
        if gen >= GEN_CAP {
            return Err(XC_BAD_GEN);
        }
        Ok(AmdBench {
            gen,
            phase: BPhase::Armed,
            items: [
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
                ItemRing::new(),
            ],
            last_run: [0; ITEM_CAP],
            red_count: 0,
        })
    }

    /// 开跑。
    pub fn start(&mut self) -> Result<(), u16> {
        if self.phase != BPhase::Armed {
            self.red_count += 1;
            return Err(XC_BAD_PHASE);
        }
        self.phase = BPhase::Running;
        Ok(())
    }

    /// 录分（代际修正后入环）。
    pub fn record(&mut self, item: usize, run: u32, raw: u32) -> Result<u32, u16> {
        if self.phase != BPhase::Running {
            self.red_count += 1;
            return Err(XC_BAD_PHASE);
        }
        if item >= ITEM_CAP {
            self.red_count += 1;
            return Err(XC_BAD_ITEM);
        }
        if raw == 0 || raw > SCORE_MAX {
            self.red_count += 1;
            return Err(XC_BAD_SCORE);
        }
        if run <= self.last_run[item] {
            self.red_count += 1;
            return Err(XC_RUN_BACKWARD);
        }
        // 代际修正（千分位系数）。
        let fact = GEN_FACTOR[self.gen] as u64;
        let mut adj = (raw as u64 * fact / 1000) as u32;
        if adj == 0 {
            adj = 1;
        }
        if adj > SCORE_MAX {
            adj = SCORE_MAX;
        }
        let ring = &mut self.items[item];
        if ring.ring.len() == RING_CAP {
            ring.ring[ring.head] = adj;
            ring.head = (ring.head + 1) % RING_CAP;
            ring.evicted += 1;
            ring.pushed += 1;
        } else {
            ring.ring.push(adj);
            ring.pushed += 1;
        }
        if adj > ring.best {
            ring.best = adj;
        }
        self.last_run[item] = run;
        Ok(adj)
    }

    /// 收跑（Running → Report；权表与环账复核）。
    pub fn finish(&mut self) -> Result<u32, u16> {
        if self.phase != BPhase::Running {
            self.red_count += 1;
            return Err(XC_BAD_PHASE);
        }
        let mut wsum: u32 = 0;
        let mut i = 0;
        while i < ITEM_CAP {
            wsum += WEIGHTS[i];
            i += 1;
        }
        if wsum != WEIGHT_SUM {
            self.red_count += 1;
            return Err(XC_WEIGHT_DRIFT);
        }
        let mut i2 = 0;
        while i2 < ITEM_CAP {
            if !self.items[i2].balanced() {
                self.red_count += 1;
                return Err(XC_RING_DRIFT);
            }
            if self.items[i2].ring.is_empty() {
                self.red_count += 1;
                return Err(XC_EMPTY_ITEM);
            }
            i2 += 1;
        }
        let total = self.total();
        self.phase = BPhase::Report;
        Ok(total)
    }

    /// 总分（加权）。
    pub fn total(&self) -> u32 {
        let mut acc: u64 = 0;
        let mut i = 0;
        while i < ITEM_CAP {
            acc += self.items[i].mean() as u64 * WEIGHTS[i] as u64;
            i += 1;
        }
        (acc / WEIGHT_SUM as u64) as u32
    }

    /// 摘要（非空判据）。
    pub fn summary(&self) -> String {
        let mut s = String::from("amdbench gen=");
        s += gen_name(self.gen);
        s += " total=";
        s += &self.total().to_string();
        s += " best0=";
        s += &self.items[0].best.to_string();
        s
    }

    /// 全账审计。
    pub fn audit(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < ITEM_CAP {
            if !self.items[i].balanced() {
                return Err(XC_RING_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 自例程：五代际跑批 → 修正分核对 → 审计 + 负例六连。
pub fn drill() -> Result<u32, u16> {
    let mut b = AmdBench::arm(3)?;
    // 负例：Armed 即录分。
    if b.record(0, 1, 50000) != Err(XC_BAD_PHASE) {
        return Err(XC_BAD_PHASE);
    }
    b.start()?;
    // 负例：Running 期 start 重复。
    if b.start() != Err(XC_BAD_PHASE) {
        return Err(XC_BAD_PHASE);
    }
    // 负例：项越界 / 分越界 / 跑批倒挂。
    if b.record(ITEM_CAP, 1, 50000) != Err(XC_BAD_ITEM) {
        return Err(XC_BAD_ITEM);
    }
    if b.record(0, 1, SCORE_MAX + 1) != Err(XC_BAD_SCORE) {
        return Err(XC_BAD_SCORE);
    }
    b.record(0, 1, 50000)?;
    if b.record(0, 1, 50000) != Err(XC_RUN_BACKWARD) {
        return Err(XC_RUN_BACKWARD);
    }
    // 八项各 3 轮。
    let mut item = 0;
    let mut run: u32 = 1;
    while item < ITEM_CAP {
        let mut k: u32 = 0;
        while k < 3 {
            run += 1;
            b.record(item, run, 40000 + item as u32 * 5000 + k * 1000)?;
            k += 1;
        }
        item += 1;
    }
    b.audit()?;
    let total = b.finish()?;
    // 负例：Report 期录分。
    if b.record(0, 9999, 50000) != Err(XC_BAD_PHASE) {
        return Err(XC_BAD_PHASE);
    }
    b.audit()?;
    // RDNA2 修正：raw 50000 → 55000。
    let mut c = AmdBench::arm(3)?;
    c.start()?;
    let adj = c.record(0, 1, 50000)?;
    if adj != 55000 {
        return Err(XC_BAD_SCORE);
    }
    // GCN4 修正：raw 50000 → 40000。
    let mut d = AmdBench::arm(0)?;
    d.start()?;
    let adj2 = d.record(0, 1, 50000)?;
    if adj2 != 40000 {
        return Err(XC_BAD_SCORE);
    }
    // 截断：RDNA3 raw 100000 → 125000 → 截 100000。
    let mut e = AmdBench::arm(4)?;
    e.start()?;
    let adj3 = e.record(0, 1, SCORE_MAX)?;
    if adj3 != SCORE_MAX {
        return Err(XC_BAD_SCORE);
    }
    // 负例：代际越界。
    if AmdBench::arm(GEN_CAP).is_ok() {
        return Err(XC_BAD_GEN);
    }
    // 负例：空项收跑。
    let mut f = AmdBench::arm(1)?;
    f.start()?;
    if f.finish() != Err(XC_EMPTY_ITEM) {
        return Err(XC_EMPTY_ITEM);
    }
    if b.summary().len() == 0 || total == 0 {
        return Err(XC_EMPTY_ITEM);
    }
    Ok(0)
}
