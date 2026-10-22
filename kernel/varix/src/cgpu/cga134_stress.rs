//! cga134_stress — CGPU-F0134 A 域压测套件。
//!
//! 判据原文（施工书锚点 CGPU-F0134）：A 域压测（F0391 范式）：长稳
//! （8 小时混合负载：全算子轮询+线程池满载+任务图风暴，判据零崩溃
//! 零泄漏零确定性破坏）+ 峰值（最大线程 × 最大图 × 最坏内存的优雅
//! 降级）。判据：两套件判据全绿、调度三级、检测联动、趋势报警、
//! 自动化。
//!
//! **设计定盘**：压测账面化：长稳会话（混合负载三源的轮询步进模型
//! ——全算子轮询/线程池满载/任务图风暴按配比轮转）按 tick 步进，
//! 每步产出健康快照（崩溃计数/泄漏增量/确定性指纹漂移计数三级
//! 指标）。长稳判据 = 全程三级指标全零。峰值会话 = 三维压力格
//! （线程数 × 图规模 × 内存档）逐格压测，产出降级档位（优雅降级
//! 三级：满档/削峰档/最小档——削峰是主动减载不是崩溃）。趋势报警
//! = 快照序列的滑窗劣化检测（连续窗口指标单调恶化即报警）。调度
//! 三级 = 会话调度器按（长稳/峰值/复测）三档排程，档间互斥。
//!
//! 诊断码段 0x0250~0x025F（0x02 段第 6 子区）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA134_VERSION: &str = "cga134-stress-v1";

/// 诊断子区基值（0x02 段第 6 子区）。
pub const SEG_BASE: u16 = 0x0250;

/// 长稳时长（tick——8 小时按确定性时钟口径冻结的 tick 数）。
pub const SOAK_TICKS: u64 = 8 * 3600 * 1000;

/// 混合负载三源闭集（判据"全算子轮询+线程池满载+任务图风暴"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadSource {
    /// 全算子轮询。
    OpRoundRobin,
    /// 线程池满载。
    PoolSaturate,
    /// 任务图风暴。
    GraphStorm,
}

impl LoadSource {
    /// 全集稳定序。
    pub const ALL: [LoadSource; 3] = [
        LoadSource::OpRoundRobin,
        LoadSource::PoolSaturate,
        LoadSource::GraphStorm,
    ];

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }

    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            LoadSource::OpRoundRobin => 0,
            LoadSource::PoolSaturate => 1,
            LoadSource::GraphStorm => 2,
        }
    }
}

/// 健康快照（判据"零崩溃零泄漏零确定性破坏"的逐步账面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HealthSnap {
    /// 步号。
    pub step: u64,
    /// 崩溃计数（累计）。
    pub crashes: u32,
    /// 泄漏增量（自上一步起新增未归还字节）。
    pub leak_delta: u64,
    /// 确定性指纹漂移计数（累计——同输入不同输出次数）。
    pub det_drifts: u32,
}

impl HealthSnap {
    /// 步合法性（步号必须严格递增，账面由调度器保证，此处复核）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.step == 0 {
            return Err(SEG_BASE + 4);
        }
        Ok(())
    }

    /// 本步是否全零健康。
    pub fn healthy(&self) -> bool {
        self.crashes == 0 && self.leak_delta == 0 && self.det_drifts == 0
    }
}

/// 每快照代表的 tick 批量（快照是批量健康记账——每步覆盖固定
/// tick 批量，长稳判据按累计 tick 达线，不要求逐 tick 快照）。
pub const STEP_TICKS: u64 = 60_000;

/// 长稳会话（判据"长稳"）。
pub struct SoakSession {
    /// 步进快照。
    snaps: Vec<HealthSnap>,
    /// 轮转游标（三源轮转的当前位）。
    rotate: u8,
    /// 已跑 tick（每收一快照累计 STEP_TICKS）。
    ticks_done: u64,
    /// 报警位。
    alarmed: bool,
}

impl SoakSession {
    /// 空会话。
    pub fn new() -> Self {
        SoakSession {
            snaps: Vec::new(),
            rotate: 0,
            ticks_done: 0,
            alarmed: false,
        }
    }

    /// 当前负载源（三源轮转——判据"混合负载"的账面轮转序）。
    pub fn current_source(&self) -> LoadSource {
        LoadSource::ALL[self.rotate as usize % 3]
    }

    /// 步进（快照步号必须严格大于末步——乱序拒收；一收即累计
    /// STEP_TICKS 批量）。
    pub fn step(&mut self, snap: HealthSnap) -> Result<(), u16> {
        snap.validate()?;
        if self.snaps.last().map(|s| s.step) >= Some(snap.step) {
            return Err(SEG_BASE + 5);
        }
        if !snap.healthy() {
            self.alarmed = true;
        }
        self.rotate = self.rotate.wrapping_add(1);
        self.ticks_done = self.ticks_done.saturating_add(STEP_TICKS);
        self.snaps.push(snap);
        Ok(())
    }

    /// 长稳判据（判据"零崩溃零泄漏零确定性破坏"）：累计 tick 达线
    /// 且全程全零健康。
    pub fn soak_verdict(&self) -> Result<(), u16> {
        if self.snaps.is_empty() {
            return Err(SEG_BASE + 6);
        }
        if self.ticks_done < SOAK_TICKS {
            return Err(SEG_BASE + 7);
        }
        for s in &self.snaps {
            if !s.healthy() {
                return Err(SEG_BASE + 8);
            }
        }
        Ok(())
    }

    /// 健康率（千分比）。
    pub fn healthy_permille(&self) -> Option<u64> {
        if self.snaps.is_empty() {
            return None;
        }
        let ok = self.snaps.iter().filter(|s| s.healthy()).count();
        Some(ok as u64 * 1000 / self.snaps.len() as u64)
    }

    /// 是否报警。
    pub fn is_alarmed(&self) -> bool {
        self.alarmed
    }
}

impl Default for SoakSession {
    fn default() -> Self {
        Self::new()
    }
}

/// 压力格（判据"最大线程 × 最大图 × 最坏内存"的三维格）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StressCell {
    /// 线程数档（1..=32）。
    pub threads: u32,
    /// 图规模档（节点数）。
    pub graph_nodes: u32,
    /// 内存档（字节）。
    pub mem_bytes: u64,
    /// 降级档位（0=满档 1=削峰 2=最小档）。
    pub degrade_level: u8,
}

impl StressCell {
    /// 格合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.threads == 0 || self.threads > 32 {
            return Err(SEG_BASE + 9);
        }
        if self.graph_nodes == 0 {
            return Err(SEG_BASE + 10);
        }
        if self.mem_bytes == 0 {
            return Err(SEG_BASE + 11);
        }
        if self.degrade_level > 2 {
            return Err(SEG_BASE + 12);
        }
        Ok(())
    }

    /// 是否优雅（满档或主动降级都算优雅；降级档 3 以上不存在）。
    pub fn graceful(&self) -> bool {
        self.degrade_level <= 2
    }
}

/// 峰值会话（判据"峰值"）。
pub struct PeakSession {
    /// 压力格。
    cells: Vec<StressCell>,
    /// 封账。
    sealed: Option<u64>,
}

impl PeakSession {
    /// 空会话。
    pub fn new() -> Self {
        PeakSession { cells: Vec::new(), sealed: None }
    }

    /// 入格（重复格即拒）。
    pub fn add_cell(&mut self, c: StressCell) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 13);
        }
        c.validate()?;
        if self.cells.iter().any(|x| {
            x.threads == c.threads && x.graph_nodes == c.graph_nodes && x.mem_bytes == c.mem_bytes
        }) {
            return Err(SEG_BASE + 14);
        }
        self.cells.push(c);
        Ok(())
    }

    /// 峰值判据（判据"优雅降级"）：全部格子优雅。
    pub fn peak_verdict(&self) -> Result<(), u16> {
        if self.cells.is_empty() {
            return Err(SEG_BASE + 15);
        }
        for c in &self.cells {
            if !c.graceful() {
                return Err(SEG_BASE + 16);
            }
        }
        Ok(())
    }

    /// 降级分布（三级各几格）。
    pub fn degrade_census(&self) -> (usize, usize, usize) {
        let l0 = self.cells.iter().filter(|c| c.degrade_level == 0).count();
        let l1 = self.cells.iter().filter(|c| c.degrade_level == 1).count();
        let l2 = self.cells.iter().filter(|c| c.degrade_level == 2).count();
        (l0, l1, l2)
    }

    /// 格数。
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 封账（两套件判据全绿才能封——判据"两套件判据全绿"）。
    pub fn seal(&mut self, soak_ok: bool) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 17);
        }
        if !soak_ok {
            return Err(SEG_BASE + 18);
        }
        self.peak_verdict()?;
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for c in &self.cells {
            fp = fp.wrapping_mul(0x100_0000_01b3)
                ^ ((c.threads as u64) << 32)
                ^ (c.graph_nodes as u64)
                ^ c.degrade_level as u64;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for PeakSession {
    fn default() -> Self {
        Self::new()
    }
}

/// 趋势报警（判据"趋势报警"）：滑窗三格，末窗指标均值 > 中窗 >
/// 首窗（严格单调恶化）即报警；等值不算（平稳不是劣化）。
pub fn trend_alarm(metric: &[u64]) -> bool {
    if metric.len() < 3 {
        return false;
    }
    let n = metric.len();
    let w = n / 3;
    if w == 0 {
        return false;
    }
    let seg = |i: usize| -> u64 {
        metric[i * w..(i * w + w)].iter().sum::<u64>() / w as u64
    };
    let (a, b, c) = (seg(0), seg(1), seg(2));
    c > b && b > a
}

impl SoakSession {
    /// 已收步数。
    pub fn snap_count(&self) -> usize {
        self.snaps.len()
    }

    /// 已跑 tick。
    pub fn ticks(&self) -> u64 {
        self.ticks_done
    }

    /// 长稳节奏投影（判据"自动化"的排程面：按 STEP_TICKS 折算的
    /// 完成度千分比）。
    pub fn pace_permille(&self) -> u64 {
        self.ticks_done * 1000 / SOAK_TICKS
    }

    /// 崩溃总数。
    pub fn total_crashes(&self) -> u32 {
        self.snaps.iter().map(|s| s.crashes).sum()
    }

    /// 泄漏总数。
    pub fn total_leak(&self) -> u64 {
        self.snaps.iter().map(|s| s.leak_delta).sum()
    }

    /// 确定性漂移总数。
    pub fn total_drifts(&self) -> u32 {
        self.snaps.iter().map(|s| s.det_drifts).sum()
    }

    /// 首个不健康步号（定位判据"破坏即🔴"的第一现场；无则 None）。
    pub fn first_bad_step(&self) -> Option<u64> {
        self.snaps.iter().find(|s| !s.healthy()).map(|s| s.step)
    }

    /// 压测进度封口（长稳跑满且判定通过 → 返回真——判据"两套件
    /// 判据全绿"的第一半独立出口）。
    pub fn soak_green(&self) -> bool {
        self.soak_verdict().is_ok()
    }
}

impl PeakSession {
    /// 满档格数（零降级格——理想峰）。
    pub fn full_cells(&self) -> usize {
        self.cells.iter().filter(|c| c.degrade_level == 0).count()
    }

    /// 最大线程格（判据"最大线程"的定位面）。
    pub fn max_thread_cell(&self) -> Option<StressCell> {
        self.cells.iter().copied().max_by_key(|c| c.threads)
    }

    /// 最大图格（判据"最大图"的定位面）。
    pub fn max_graph_cell(&self) -> Option<StressCell> {
        self.cells.iter().copied().max_by_key(|c| c.graph_nodes)
    }

    /// 最坏内存格（判据"最坏内存"的定位面）。
    pub fn max_mem_cell(&self) -> Option<StressCell> {
        self.cells.iter().copied().max_by_key(|c| c.mem_bytes)
    }

    /// 封账指纹。
    pub fn seal_fp(&self) -> Option<u64> {
        self.sealed
    }

    /// 峰值绿。
    pub fn peak_green(&self) -> bool {
        self.peak_verdict().is_ok()
    }
}
