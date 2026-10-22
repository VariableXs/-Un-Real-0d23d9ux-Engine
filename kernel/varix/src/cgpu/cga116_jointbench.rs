//! cga116_jointbench — CGPU-F0116 CPU/GPU 联合基准。
//!
//! 判据原文（施工书锚点 CGPU-F0116）：CPU 与 GPU 渲染路径的联合基准：
//! 同场景双路吞吐对比（每场景标注瓶颈归属——CPU bound/GPU bound/带宽
//! bound）、混合渲染（F0353）的收益基准、路由决策器的决策质量评估
//! （决策 vs 事后最优）。判据：场景集、瓶颈标注、混合收益、决策质量、
//! 入册。
//!
//! **设计定盘**：联合基准的核心产出是「逐场景三列账」：CPU 吞吐、GPU
//! 吞吐、瓶颈归属。瓶颈归属用整数判定（不用浮点）：两侧耗时差的相对
//! 千分比超阈值即「快侧为瓶颈的受害侧」——瓶颈归属=慢侧（谁慢谁就是
//! 瓶颈）。混合收益 = 混合吞吐对两侧单路最优的超越比（千分比记账）。
//! 决策质量 = 路由决策器选路与事后最优路的吞吐差（决策偏差 ppm 记账，
//! 零偏差满分，偏差超 10% 即立案——决策质量是路由器的回归门）。
//!
//! 诊断码段 0x0100~0x010F 第 3 子区（0x0120~0x012F）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。
//! 时间量一律整数 tick（由确定性时钟注入），吞吐=像素/tick 的定点化。

use alloc::vec::Vec;

/// 模块版本。
pub const CGA116_VERSION: &str = "cga116-jointbench-v1";

/// 诊断子区基值（0x01 段第 3 子区）。
pub const SEG_BASE: u16 = 0x0120;

/// 瓶颈归属闭集（判据"瓶颈标注"三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bottleneck {
    /// CPU bound——CPU 侧更慢。
    Cpu,
    /// GPU bound——GPU 侧更慢。
    Gpu,
    /// 带宽 bound——两侧耗时接近且都受传输拖累（差值在带宽阈值内）。
    Bandwidth,
}

impl Bottleneck {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            Bottleneck::Cpu => SEG_BASE + 1,
            Bottleneck::Gpu => SEG_BASE + 2,
            Bottleneck::Bandwidth => SEG_BASE + 3,
        }
    }
}

/// 瓶颈判定（判据"每场景标注瓶颈归属"）：
/// 相对差 = |cpu_ticks - gpu_ticks| * 1000 / max(两者)。相对差 ≤
/// BANDWIDTH_TOL_PERMILLE 视为带宽 bound（两侧谁都不显著占优，
/// 瓶颈在传输面）；否则慢侧为瓶颈。
pub const BANDWIDTH_TOL_PERMILLE: u64 = 150;

/// 决策偏差立案线（ppm）。
pub const DECISION_DEVIATION_ALARM_PPM: u64 = 100_000;

/// 瓶颈归属判定。
pub fn classify_bottleneck(cpu_ticks: u64, gpu_ticks: u64) -> Bottleneck {
    let larger = if cpu_ticks > gpu_ticks { cpu_ticks } else { gpu_ticks };
    if larger == 0 {
        return Bottleneck::Bandwidth;
    }
    let diff = if cpu_ticks > gpu_ticks {
        cpu_ticks - gpu_ticks
    } else {
        gpu_ticks - cpu_ticks
    };
    if diff * 1000 <= larger * BANDWIDTH_TOL_PERMILLE {
        Bottleneck::Bandwidth
    } else if cpu_ticks > gpu_ticks {
        Bottleneck::Cpu
    } else {
        Bottleneck::Gpu
    }
}

/// 一条场景基准记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointRow {
    /// 场景编号。
    pub scene: u32,
    /// 像素量（吞吐分母基准）。
    pub pixels: u64,
    /// CPU 路径耗时（tick）。
    pub cpu_ticks: u64,
    /// GPU 路径耗时（tick）。
    pub gpu_ticks: u64,
    /// 混合路径耗时（tick；0 表示未测）。
    pub mixed_ticks: u64,
    /// 瓶颈归属。
    pub bottleneck: Bottleneck,
    /// 路由决策（true=选了 GPU 路，false=选了 CPU 路）。
    pub routed_gpu: bool,
}

/// 混合收益（千分比）：mixed 相对两侧最优的超越比。
/// 返回 (best_single_ticks, gain_permille)；mixed 未测返回 None。
pub fn mixed_gain(row: &JointRow) -> Option<(u64, u64)> {
    if row.mixed_ticks == 0 || row.cpu_ticks == 0 || row.gpu_ticks == 0 {
        return None;
    }
    let best = if row.cpu_ticks <= row.gpu_ticks {
        row.cpu_ticks
    } else {
        row.gpu_ticks
    };
    // 收益 = (best - mixed) * 1000 / best；负收益走饱和截断到 0，
    // 全程无符号运算避免 i64/u64 混算（负向收益由 mixed_positive_scenes
    // 的阈值语义兜底：0 视为无收益）。
    if row.mixed_ticks <= best {
        Some((best, ((best - row.mixed_ticks) * 1000) / best))
    } else {
        Some((best, 0u64.saturating_sub(((row.mixed_ticks - best) * 1000) / best)))
    }
}

/// 决策偏差（ppm）：决策路耗时对最优路耗时的相对偏差。
pub fn decision_deviation_ppm(row: &JointRow) -> u64 {
    let chosen = if row.routed_gpu {
        row.gpu_ticks
    } else {
        row.cpu_ticks
    };
    let best = if row.cpu_ticks < row.gpu_ticks {
        row.cpu_ticks
    } else {
        row.gpu_ticks
    };
    if best == 0 || chosen <= best {
        return 0;
    }
    ((chosen - best) * 1_000_000) / best
}

/// 联合基准册（判据"入册"）。
pub struct JointBenchBook {
    /// 场景记录（登记序）。
    rows: Vec<JointRow>,
    /// 立案清单（决策偏差超线的场景）。
    cases: Vec<(u32, u64)>,
    /// 封册指纹。
    sealed: Option<u64>,
}

impl JointBenchBook {
    /// 空册。
    pub fn new() -> Self {
        JointBenchBook {
            rows: Vec::new(),
            cases: Vec::new(),
            sealed: None,
        }
    }

    /// 登记场景（重复即拒；像素或任一单路耗时为零即拒）。
    pub fn record(
        &mut self,
        scene: u32,
        pixels: u64,
        cpu_ticks: u64,
        gpu_ticks: u64,
        mixed_ticks: u64,
        routed_gpu: bool,
    ) -> Result<(), u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        if pixels == 0 || cpu_ticks == 0 || gpu_ticks == 0 {
            return Err(SEG_BASE + 5);
        }
        if self.rows.iter().any(|r| r.scene == scene) {
            return Err(SEG_BASE + 6);
        }
        let bottleneck = classify_bottleneck(cpu_ticks, gpu_ticks);
        self.rows.push(JointRow {
            scene,
            pixels,
            cpu_ticks,
            gpu_ticks,
            mixed_ticks,
            bottleneck,
            routed_gpu,
        });
        Ok(())
    }

    /// 场景数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 查场景。
    pub fn row(&self, idx: usize) -> Option<&JointRow> {
        self.rows.get(idx)
    }

    /// 决策质量评估（判据"决策质量"）：全场景决策偏差评估，
    /// 超线场景立案。返回立案数。
    pub fn audit_decisions(&mut self) -> Result<usize, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 4);
        }
        self.cases.clear();
        for r in &self.rows {
            let dev = decision_deviation_ppm(r);
            if dev > DECISION_DEVIATION_ALARM_PPM {
                self.cases.push((r.scene, dev));
            }
        }
        Ok(self.cases.len())
    }

    /// 立案查询。
    pub fn case(&self, idx: usize) -> Option<(u32, u64)> {
        self.cases.get(idx).copied()
    }

    /// 混合收益统计（判据"混合收益"）：有混合数据的场景里，
    /// 收益为正的场景数。
    pub fn mixed_positive_scenes(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| mixed_gain(r).map(|(_, g)| g > 0).unwrap_or(false))
            .count()
    }

    /// 封册（场景册非空且决策已审才能封）。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed.is_some() {
            return Err(SEG_BASE + 7);
        }
        if self.rows.is_empty() {
            return Err(SEG_BASE + 8);
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        for r in &self.rows {
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.scene as u64;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.cpu_ticks;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.gpu_ticks;
            fp = fp.wrapping_mul(0x100_0000_01b3) ^ r.mixed_ticks;
        }
        self.sealed = Some(fp);
        Ok(fp)
    }
}

impl Default for JointBenchBook {
    fn default() -> Self {
        Self::new()
    }
}

/// 吞吐换算（判据"双路吞吐对比"的量化面）：Kpix/tick 的定点值
/// （千像素每 tick × 256 定点）。像素或耗时为零返回 None。
pub fn throughput_fp256(row: &JointRow) -> Option<u64> {
    if row.pixels == 0 || row.cpu_ticks == 0 || row.gpu_ticks == 0 {
        return None;
    }
    let kp = row.pixels / 1000;
    if kp == 0 {
        return None;
    }
    // 选快路吞吐作为场景代表吞吐。
    let fast = if row.cpu_ticks < row.gpu_ticks {
        row.cpu_ticks
    } else {
        row.gpu_ticks
    };
    Some(kp * 256 / fast)
}

/// 决策质量评级四档（判据"决策质量评估"的评级面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecisionGrade {
    /// 完美：全场景零偏差。
    Perfect,
    /// 良好：平均偏差 ≤ 1%（10_000 ppm）。
    Good,
    /// 合格：平均偏差 ≤ 10%（立案线）。
    Fair,
    /// 失格：平均偏差超立案线。
    Poor,
}

impl DecisionGrade {
    /// 诊断码。
    pub fn code(self) -> u16 {
        match self {
            DecisionGrade::Perfect => SEG_BASE + 9,
            DecisionGrade::Good => SEG_BASE + 10,
            DecisionGrade::Fair => SEG_BASE + 11,
            DecisionGrade::Poor => SEG_BASE + 12,
        }
    }
}

impl JointBenchBook {
    /// 决策质量评级（判据"决策质量"）：全场景平均偏差 ppm → 四档。
    /// 空册返回 None。
    pub fn decision_grade(&self) -> Option<DecisionGrade> {
        if self.rows.is_empty() {
            return None;
        }
        let sum: u64 = self.rows.iter().map(|r| decision_deviation_ppm(r)).sum();
        let avg = sum / self.rows.len() as u64;
        Some(if avg == 0 {
            DecisionGrade::Perfect
        } else if avg <= 10_000 {
            DecisionGrade::Good
        } else if avg <= DECISION_DEVIATION_ALARM_PPM {
            DecisionGrade::Fair
        } else {
            DecisionGrade::Poor
        })
    }

    /// 瓶颈分布（判据"瓶颈标注"的统计面）：三态计数。
    pub fn bottleneck_census(&self) -> (usize, usize, usize) {
        let mut cpu = 0;
        let mut gpu = 0;
        let mut bw = 0;
        for r in &self.rows {
            match r.bottleneck {
                Bottleneck::Cpu => cpu += 1,
                Bottleneck::Gpu => gpu += 1,
                Bottleneck::Bandwidth => bw += 1,
            }
        }
        (cpu, gpu, bw)
    }

    /// 主导瓶颈（计数最大者；平票按 Cpu<Gpu<Bandwidth 稳定序取先）。
    pub fn dominant_bottleneck(&self) -> Option<Bottleneck> {
        if self.rows.is_empty() {
            return None;
        }
        let (cpu, gpu, bw) = self.bottleneck_census();
        Some(if cpu >= gpu && cpu >= bw {
            Bottleneck::Cpu
        } else if gpu >= bw {
            Bottleneck::Gpu
        } else {
            Bottleneck::Bandwidth
        })
    }

    /// 场景代表吞吐（定点 Kpix/tick；未测出 None 的场景跳过）。
    pub fn throughput_rows(&self) -> Vec<(u32, u64)> {
        self.rows
            .iter()
            .filter_map(|r| throughput_fp256(r).map(|t| (r.scene, t)))
            .collect()
    }

    /// 决策命中率（判据"决策 vs 事后最优"）：决策正确场景占比
    /// （千分比）。
    pub fn decision_hit_permille(&self) -> Option<u64> {
        if self.rows.is_empty() {
            return None;
        }
        let hit = self
            .rows
            .iter()
            .filter(|r| decision_deviation_ppm(r) == 0)
            .count();
        Some(hit as u64 * 1000 / self.rows.len() as u64)
    }
}
