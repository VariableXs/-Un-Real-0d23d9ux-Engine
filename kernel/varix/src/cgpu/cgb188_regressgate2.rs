//! CGPU-F0188 · 图性能回归门（判据段 0x0560）
//! 图构建与执行的回归门：图结构基线（典型帧的图结构快照——节点数/
//! 边数/关键路径长度/每队列任务分布的基线值），变更检测（图结构/
//! 执行耗时的回归检测——结构膨胀/耗时劣化即门禁红）。与 F0170 缓存
//! 联动（图缓存命中变化也入基线监控）。判据：基线定义、回归检测、
//! 阈值校准、CI 集成、报告。
//!
//! 诊断码段 0x0560（本模块独占：0x0560~0x056F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0560;

/// 结构回归容忍带宽（千分比）：结构指标 ±15% 内视为波动。
pub const STRUCT_TOLERANCE_PERMILLE: u32 = 150;

/// 耗时回归容忍带宽（千分比）：耗时 +10% 内视为抖动。
pub const TIME_TOLERANCE_PERMILLE: u32 = 100;

/// 图结构基线（判据「基线定义」载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StructBaseline {
    /// 场景号。
    pub scene: u32,
    /// 节点数。
    pub nodes: u32,
    /// 边数。
    pub edges: u32,
    /// 关键路径长度。
    pub critical: u32,
    /// 图形队列任务数。
    pub gfx_tasks: u32,
    /// 计算队列任务数。
    pub cmp_tasks: u32,
}

impl StructBaseline {
    /// 基线校验：场景非零、账面自洽（节点 = 两队列任务合计、
    /// 边 ≥ 节点-1）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.scene == 0 || self.nodes == 0 {
            return Err(SEG_BASE);
        }
        if self.gfx_tasks + self.cmp_tasks != self.nodes {
            return Err(SEG_BASE + 1);
        }
        if (self.edges as u64) < (self.nodes as u64).saturating_sub(1) {
            return Err(SEG_BASE + 2);
        }
        if self.critical == 0 || self.critical > self.nodes {
            return Err(SEG_BASE + 3);
        }
        Ok(())
    }
}

/// 基线库（判据「基线定义」容器）：每场景一条基线。
pub struct BaselineBook {
    /// 基线集（场景唯一）。
    pub baselines: Vec<StructBaseline>,
    /// 已冻结（冻结后基线只读）。
    pub frozen: bool,
}

impl BaselineBook {
    /// 空库。
    pub fn new() -> Self {
        BaselineBook {
            baselines: Vec::new(),
            frozen: false,
        }
    }

    /// 录基线（场景重复拒；冻结后拒）。
    pub fn record(&mut self, b: StructBaseline) -> Result<(), u16> {
        b.validate()?;
        if self.frozen {
            return Err(SEG_BASE + 4);
        }
        if self.baselines.iter().any(|x| x.scene == b.scene) {
            return Err(SEG_BASE + 5);
        }
        self.baselines.push(b);
        Ok(())
    }

    /// 冻结（判据「CI 集成」前置：CI 用的基线必须冻结——防止
    /// CI 期间基线漂移）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen || self.baselines.is_empty() {
            return Err(SEG_BASE + 6);
        }
        self.frozen = true;
        Ok(())
    }

    /// 查基线。
    pub fn get(&self, scene: u32) -> Result<&StructBaseline, u16> {
        self.baselines
            .iter()
            .find(|b| b.scene == scene)
            .ok_or(SEG_BASE + 7)
    }
}

/// 结构回归检测（判据「回归检测」核心面）：观测 vs 基线逐项
/// 对比——节点/边/关键路径任一项超带宽即红，返回违约项清单。
pub fn structural_regression(
    base: &StructBaseline,
    obs: &StructBaseline,
) -> Result<Vec<u8>, u16> {
    if base.scene != obs.scene {
        return Err(SEG_BASE + 8);
    }
    obs.validate()?;
    let mut bad: Vec<u8> = Vec::new();
    let check = |b: u32, o: u32| -> bool {
        let lo = (b as u64 * (1000 - STRUCT_TOLERANCE_PERMILLE as u64) / 1000) as u32;
        let hi = (b as u64 * (1000 + STRUCT_TOLERANCE_PERMILLE as u64) / 1000) as u32;
        o < lo || o > hi
    };
    if check(base.nodes, obs.nodes) {
        bad.push(0);
    }
    if check(base.edges, obs.edges) {
        bad.push(1);
    }
    if check(base.critical, obs.critical) {
        bad.push(2);
    }
    if !bad.is_empty() {
        return Err(SEG_BASE + 9);
    }
    Ok(bad)
}

/// 耗时回归检测（判据「回归检测」耗时面）：观测耗时 vs 基线
/// 耗时——涨幅超 10% 即红（千分比带宽 TIME_TOLERANCE）。
pub fn time_regression(base_us: u64, obs_us: u64) -> Result<u32, u16> {
    if base_us == 0 || obs_us == 0 {
        return Err(SEG_BASE + 10);
    }
    let growth = ((obs_us.saturating_sub(base_us)) * 1000 / base_us) as u32;
    if growth > TIME_TOLERANCE_PERMILLE {
        return Err(SEG_BASE + 11);
    }
    Ok(growth)
}

/// 缓存命中联动监控（判据「与 F0170 缓存联动」面）：缓存命中率
/// 相邻两窗跌幅超 20% = 图结构隐性变化（基线外的软信号）。
pub fn cache_hit_watch(prev_permille: u32, cur_permille: u32) -> Result<bool, u16> {
    if prev_permille > 1000 || cur_permille > 1000 {
        return Err(SEG_BASE + 12);
    }
    let drop = prev_permille.saturating_sub(cur_permille);
    Ok(drop * 5 > prev_permille)
}

/// 门禁报告（判据「报告」载体）。
pub struct GateReport {
    /// 场景号。
    pub scene: u32,
    /// 结构判绿。
    pub struct_ok: bool,
    /// 耗时涨幅（千分比）。
    pub time_growth: u32,
    /// 缓存软信号。
    pub cache_soft: bool,
    /// 结论（true=过门 false=拦下）。
    pub passed: bool,
}

impl GateReport {
    /// 报告校验：结论与分项一致（分项全绿才可过——报告不可自欺）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.scene == 0 {
            return Err(SEG_BASE + 13);
        }
        let expect = self.struct_ok && !self.cache_soft && self.time_growth <= TIME_TOLERANCE_PERMILLE;
        if self.passed != expect {
            return Err(SEG_BASE + 14);
        }
        Ok(())
    }
}

/// 门禁评估（判据「CI 集成」核心面）：一次完整过门——结构回归 +
/// 耗时回归 + 缓存联动，产出报告。
pub fn gate_evaluate(
    book: &BaselineBook,
    obs: &StructBaseline,
    base_us: u64,
    obs_us: u64,
    prev_cache: u32,
    cur_cache: u32,
) -> Result<GateReport, u16> {
    if !book.frozen {
        return Err(SEG_BASE + 15);
    }
    let base = book.get(obs.scene)?;
    let struct_ok = structural_regression(base, obs).is_ok();
    let tg = time_regression(base_us, obs_us)?;
    let soft = cache_hit_watch(prev_cache, cur_cache)?;
    let r = GateReport {
        scene: obs.scene,
        struct_ok,
        time_growth: tg,
        cache_soft: soft,
        passed: struct_ok && !soft && tg <= TIME_TOLERANCE_PERMILLE,
    };
    r.validate()?;
    Ok(r)
}

/// 阈值校准审计（判据「阈值校准」面）：结构带宽必须宽于耗时带宽
/// （结构波动天然大于耗时抖动——带宽倒挂 = 校准错误）。
pub fn threshold_calibration() -> Result<(), u16> {
    if STRUCT_TOLERANCE_PERMILLE <= TIME_TOLERANCE_PERMILLE {
        return Err(SEG_BASE + 16);
    }
    Ok(())
}

/// 基线库深度扩展面。
impl BaselineBook {
    /// 基线规模。
    pub fn count(&self) -> usize {
        self.baselines.len()
    }

    /// 基线指纹（判据「基线定义」可算化面）：全部基线折并——
    /// 冻结前后的指纹不一致 = 基线被偷改。
    pub fn book_fp(&self) -> Result<u64, u16> {
        if self.baselines.is_empty() {
            return Err(SEG_BASE + 17);
        }
        let mut acc: u64 = 0xcbf29ce484222325;
        for b in &self.baselines {
            acc = acc.wrapping_mul(0x100000001b3) ^ b.scene as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ b.nodes as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ b.edges as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ b.critical as u64;
        }
        Ok(acc)
    }
}

/// 连续门禁演练（判据「CI 集成」批量面）：多场景连续过门，
/// 全绿才算 CI 就绪（单场景过门不构成 CI 证据）。
pub fn gate_batch(book: &BaselineBook, samples: &[StructBaseline]) -> Result<usize, u16> {
    if !book.frozen {
        return Err(SEG_BASE + 18);
    }
    if samples.len() < 3 {
        return Err(SEG_BASE + 19);
    }
    let mut passed = 0usize;
    for s in samples {
        let base_us = 1000u64;
        let r = gate_evaluate(book, s, base_us, base_us + 50, 900, 890)?;
        if r.passed {
            passed += 1;
        }
    }
    if passed != samples.len() {
        return Err(SEG_BASE + 20);
    }
    Ok(passed)
}

/// 门禁报告深度扩展面。
impl GateReport {
    /// 违约清单（判据「报告」可行动面）：报告不通过时给出违约项
    /// 编号（0=结构 1=耗时 2=缓存软信号）——CI 摘要直接可读。
    pub fn violations(&self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        if !self.struct_ok {
            out.push(0);
        }
        if self.time_growth > TIME_TOLERANCE_PERMILLE {
            out.push(1);
        }
        if self.cache_soft {
            out.push(2);
        }
        out
    }

    /// 报告指纹（判据「CI 集成」可审计面）：报告折并——CI 日志
    /// 里的报告不可篡改（指纹对账）。
    pub fn report_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.scene as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ self.time_growth as u64;
        acc = acc.wrapping_mul(0x100000001b3) ^ (self.passed as u64);
        acc
    }
}

/// 结构回归深度扩展（独立函数面）：逐项涨幅投影（千分比，可为负
/// ——负值 = 缩水，结构缩水也报警：可能是图被误删）。
pub fn structural_deltas(base: &StructBaseline, obs: &StructBaseline) -> Result<(i64, i64, i64), u16> {
    if base.scene != obs.scene {
        return Err(SEG_BASE + 21);
    }
    base.validate()?;
    obs.validate()?;
    let delta = |b: u32, o: u32| -> i64 {
        ((o as i64 - b as i64) * 1000) / b.max(1) as i64
    };
    Ok((
        delta(base.nodes, obs.nodes),
        delta(base.edges, obs.edges),
        delta(base.critical, obs.critical),
    ))
}

/// 门禁历史台账（独立函数面）：连续 N 次过门记录的稳定性判
/// ——全过 + 无缓存软信号才稳定（过门率 100% 是 CI 基线纪律）。
pub fn gate_history_stable(reports: &[GateReport]) -> Result<u32, u16> {
    if reports.len() < 5 {
        return Err(SEG_BASE + 22);
    }
    for r in reports {
        r.validate()?;
        if !r.passed {
            return Err(SEG_BASE + 23);
        }
    }
    let soft = reports.iter().filter(|r| r.cache_soft).count();
    if soft > 0 {
        return Err(SEG_BASE + 24);
    }
    Ok(reports.len() as u32)
}
