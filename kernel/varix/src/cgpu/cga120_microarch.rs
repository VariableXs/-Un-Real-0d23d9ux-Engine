//! cga120_microarch — CGPU-F0120 同指令集微架构调优。
//!
//! 判据原文（施工书锚点 CGPU-F0120）：同指令集内不同微架构的性能差异
//! 调优：分派器细化（SKY 档/ICE 档/ZEN 档——按 CPU 家族细分子路径：同
//! AVX2 但端口宽度不同）、调优依据（微架构手册的端口约束知识库化）、
//! 调优收益实测入册。判据：子路径分派、知识库、收益实测、家族覆盖、
//! 回归。
//!
//! **设计定盘**：分派模型 =（指令集档位 × 家族档位）→ 子路径号。
//! 指令集档位闭集（Sse2 / Avx2 / Avx512）× 家族档位闭集（Sky = 宽端口
//! 大核 / Ice = 窄端口小核 / Zen = AMD 宽端口）= 9 格分派矩阵，每格
//! 显性给出子路径号或 NotTuned（未调优格诚实显性，不静默回退到默认——
//! 未调优是事实，静默回退是谎言）。知识库 = 端口约束条目册：每条目
//! （家族档 × 端口资源 → 每周期吞吐上限），调优依据可查。收益实测 =
//! 调优前后同负载耗时差（千分比），负收益即回退登记。
//!
//! 诊断码段 0x0100~0x010F 第 7 子区（0x0160~0x016F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA120_VERSION: &str = "cga120-microarch-v1";

/// 诊断子区基值（0x01 段第 7 子区）。
pub const SEG_BASE: u16 = 0x0160;

/// 指令集档位闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IsaTier {
    /// SSE2 基线。
    Sse2,
    /// AVX2。
    Avx2,
    /// AVX512。
    Avx512,
}

impl IsaTier {
    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            IsaTier::Sse2 => 0,
            IsaTier::Avx2 => 1,
            IsaTier::Avx512 => 2,
        }
    }
}

/// CPU 家族档位闭集（判据"SKY 档/ICE 档/ZEN 档"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FamilyTier {
    /// SKY：宽端口大核。
    Sky,
    /// ICE：窄端口小核。
    Ice,
    /// ZEN：AMD 宽端口。
    Zen,
}

impl FamilyTier {
    /// 稳定序号。
    pub fn ordinal(self) -> u8 {
        match self {
            FamilyTier::Sky => 0,
            FamilyTier::Ice => 1,
            FamilyTier::Zen => 2,
        }
    }

    /// 诊断码。
    pub fn code(self) -> u16 {
        SEG_BASE + self.ordinal() as u16 + 1
    }
}

/// 分派结果：子路径号或诚实未调优。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dispatch {
    /// 已调优子路径号（1 起）。
    Tuned(u16),
    /// 未调优（显性事实，下游走基线路径并知晓这一点）。
    NotTuned,
}

/// 分派矩阵（判据"子路径分派"——9 格显性）。
pub struct DispatchMatrix {
    /// 行=IsaTier 序，列=FamilyTier 序；None=未调优。
    grid: [[Option<u16>; 3]; 3],
}

impl DispatchMatrix {
    /// 空矩阵（全未调优）。
    pub fn new() -> Self {
        DispatchMatrix { grid: [[None; 3]; 3] }
    }

    /// 登记子路径（路径号 0 即拒——0 保留为基线语义；重复登记同格
    /// 换值即拒，分派表是冻结面不是配置面）。
    pub fn set(
        &mut self,
        isa: IsaTier,
        fam: FamilyTier,
        path: u16,
    ) -> Result<(), u16> {
        if path == 0 {
            return Err(SEG_BASE + 4);
        }
        let slot = &mut self.grid[isa.ordinal() as usize][fam.ordinal() as usize];
        if slot.is_some() {
            return Err(SEG_BASE + 5);
        }
        *slot = Some(path);
        Ok(())
    }

    /// 分派（全格显性：未调优返回 NotTuned 而非猜默认）。
    pub fn dispatch(&self, isa: IsaTier, fam: FamilyTier) -> Dispatch {
        match self.grid[isa.ordinal() as usize][fam.ordinal() as usize] {
            Some(p) => Dispatch::Tuned(p),
            None => Dispatch::NotTuned,
        }
    }

    /// 调优覆盖格数。
    pub fn tuned_cells(&self) -> usize {
        self.grid.iter().flatten().filter(|s| s.is_some()).count()
    }

    /// 家族覆盖（判据"家族覆盖"）：每个家族至少一个已调优格。
    pub fn family_coverage(&self) -> Result<(), u16> {
        for f in [FamilyTier::Sky, FamilyTier::Ice, FamilyTier::Zen] {
            let hit = (0..3).any(|i| {
                self.grid[i][f.ordinal() as usize].is_some()
            });
            if !hit {
                return Err(f.code());
            }
        }
        Ok(())
    }
}

impl Default for DispatchMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// 端口约束知识库条目（判据"知识库"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PortEntry {
    /// 家族档。
    pub fam: FamilyTier,
    /// 端口资源名（如 "FP_MUL"）。
    pub port: &'static str,
    /// 每周期吞吐上限（1/256 定点：256=每周期 1 次）。
    pub per_cycle_fp256: u32,
}

impl PortEntry {
    /// 条目合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.per_cycle_fp256 == 0 || self.per_cycle_fp256 > 256 * 8 {
            return Err(SEG_BASE + 6);
        }
        Ok(())
    }
}

/// 知识库。
pub struct PortKnowledge {
    /// 条目册。
    entries: Vec<PortEntry>,
}

impl PortKnowledge {
    /// 空库。
    pub fn new() -> Self {
        PortKnowledge { entries: Vec::new() }
    }

    /// 入库（同家族同端口重复即拒——知识库事实面不可覆盖写）。
    pub fn ingest(&mut self, e: PortEntry) -> Result<(), u16> {
        e.validate()?;
        if self
            .entries
            .iter()
            .any(|q| q.fam == e.fam && q.port == e.port)
        {
            return Err(SEG_BASE + 7);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 查询（家族 + 端口）。
    pub fn lookup(&self, fam: FamilyTier, port: &'static str) -> Option<&PortEntry> {
        self.entries
            .iter()
            .find(|e| e.fam == fam && e.port == port)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空库判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 收益实测条目（判据"收益实测入册"+"回归"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GainRow {
    /// 家族档。
    pub fam: FamilyTier,
    /// 负载编号。
    pub load: u32,
    /// 调优前耗时（tick）。
    pub before_ticks: u64,
    /// 调优后耗时（tick）。
    pub after_ticks: u64,
}

/// 收益账。
pub struct GainBook {
    /// 实测条目。
    rows: Vec<GainRow>,
    /// 回退登记（负收益条目）。
    reverted: Vec<(FamilyTier, u32)>,
    /// 封账。
    sealed: Option<u64>,
}

impl GainBook {
    /// 空账。
    pub fn new() -> Self {
        GainBook {
            rows: Vec::new(),
            reverted: Vec::new(),
            sealed: None,
        }
    }

    /// 收益千分比：(before - after) * 1000 / before。正=变快。
    pub fn gain_permille(row: &GainRow) -> Result<i64, u16> {
        if row.before_ticks == 0 || row.after_ticks == 0 {
            return Err(SEG_BASE + 8);
        }
        if row.after_ticks <= row.before_ticks {
            Ok(((row.before_ticks - row.after_ticks) * 1000 / row.before_ticks) as i64)
        } else {
            Ok(-(((row.after_ticks - row.before_ticks) * 1000 / row.before_ticks) as i64))
        }
    }

    /// 入账（同家族同负载重复即拒；负收益自动进回退登记——
    /// 回归防线：调劣了的路径必须显性回退）。
    pub fn record(&mut self, row: GainRow) -> Result<i64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 9);
        }
        if self
            .rows
            .iter()
            .any(|r| r.fam == row.fam && r.load == row.load)
        {
            return Err(SEG_BASE + 10);
        }
        let g = Self::gain_permille(&row)?;
        if g < 0 {
            self.reverted.push((row.fam, row.load));
        }
        self.rows.push(row);
        Ok(g)
    }

    /// 回退登记数。
    pub fn revert_count(&self) -> usize {
        self.reverted.len()
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 11);
        }
        if self.rows.is_empty() {
            return Err(SEG_BASE + 12);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.load as u64;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.after_ticks;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for GainBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 核心端口闭集（判据"知识库"的覆盖核验面）：三家族每家至少
/// 登记这四个端口资源。
pub const CORE_PORTS: [&str; 4] = ["FP_ADD", "FP_MUL", "LOAD", "STORE"];

impl PortKnowledge {
    /// 端口覆盖核验（判据"知识库"）：三家族 × 核心端口全在册。
    pub fn core_coverage(&self) -> Result<(), u16> {
        for fam in [FamilyTier::Sky, FamilyTier::Ice, FamilyTier::Zen] {
            for port in CORE_PORTS {
                if self.lookup(fam, port).is_none() {
                    return Err(SEG_BASE + 13);
                }
            }
        }
        Ok(())
    }

    /// 家族条目数。
    pub fn fam_count(&self, fam: FamilyTier) -> usize {
        self.entries.iter().filter(|e| e.fam == fam).count()
    }
}

impl DispatchMatrix {
    /// 调优完整度投影（判据"子路径分派"的账面）：9 格逐格
    /// (IsaTier 序, FamilyTier 序, 子路径号或 0=未调优)。
    pub fn grid_projection(&self) -> Vec<(u8, u8, u16)> {
        let mut out = Vec::new();
        for i in 0..3u8 {
            for f in 0..3u8 {
                let p = self.grid[i as usize][f as usize].unwrap_or(0);
                out.push((i, f, p));
            }
        }
        out
    }

    /// 同指令集跨家族覆盖差（判据"家族覆盖"的量化面）：
    /// 返回每家族已调优格数。
    pub fn fam_coverage_counts(&self) -> [usize; 3] {
        let mut out = [0usize; 3];
        for f in 0..3usize {
            out[f] = (0..3).filter(|&i| self.grid[i][f].is_some()).count();
        }
        out
    }
}

impl GainBook {
    /// 域聚合（判据"收益实测入册"的聚合面）：全部条目的
    /// 平均收益千分比（空账 None；负收益条目参与均值——
    /// 回退条目不能被藏起来）。
    pub fn avg_gain_permille(&self) -> Option<i64> {
        if self.rows.is_empty() {
            return None;
        }
        let mut sum: i64 = 0;
        for r in &self.rows {
            sum += match Self::gain_permille(r) { Ok(v) => v, Err(_) => return None };
        }
        Some(sum / self.rows.len() as i64)
    }

    /// 正收益占比（千分比；空账 None）。
    pub fn positive_ratio_permille(&self) -> Option<u64> {
        if self.rows.is_empty() {
            return None;
        }
        let pos = self
            .rows
            .iter()
            .filter(|r| Self::gain_permille(r).unwrap_or(-1) > 0)
            .count();
        Some(pos as u64 * 1000 / self.rows.len() as u64)
    }

    /// 条目数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}
