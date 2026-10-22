//! VE-F0487 · 寄存器压力预估模型（VE-C 域 · 优化组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0487`
//!
//! **锚点原文**：「寄存器压力预估模型：活跃值规模估算（每点活跃变量数按指
//! 令序扫描）、压力热图（函数内压力曲线）、超限预警（超目标寄存器数的区间
//! 标注与降级建议——减少同时活跃的提示给上游）；模型参数显性可调。判据：
//! 活跃扫描、压力热图、超限预警、模型校准、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **活跃扫描**（判据一）。指令序扫描（def 进活跃集、last use 出——活
//!    跃值数逐点曲线）。
//!
//! 2. **压力热图**（判据二）。压力曲线（逐点活跃数 + 峰值标注——曲线面
//!    可查 [`PressureMap::peak`]）。
//!
//! 3. **超限预警**（判据三）。超目标寄存器数区间标注 + 降级建议（减少同时
//!    活跃——提示给上游），预警限频合并（风暴面）。
//!
//! 4. **模型校准**（判据四）。模型参数显性可调（每值寄存器占用 [`ModelParam`]），
//!    校准记录限频（偏差大 → 校准账）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0487";

/// 模型参数（显性可调：每值占用寄存器数）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelParam {
    /// 每活跃值的寄存器占用（标量 1 / 向量 4——调用方按值类型给）。
    pub regs_per_value: u32,
    /// 目标寄存器数（超限预警基准）。
    pub target_regs: u32,
}

impl ModelParam {
    /// 参数合法性（非法 → 拒绝保留当前——锚点面）。
    pub fn valid(&self) -> bool {
        self.regs_per_value >= 1 && self.target_regs >= 1
    }
}

/// 压力点（指令序位置 → 压力值）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressurePoint {
    pub at: usize,
    pub live_vals: usize,
    pub regs: u32,
}

/// 压力热图。
#[derive(Debug, Default)]
pub struct PressureMap {
    pub curve: Vec<PressurePoint>,
    pub target_regs: u32,
    /// 超限区间标注（[起, 止]）。
    pub hot_zones: Vec<(usize, usize)>,
    /// 预警记录（限频合并后）。
    pub warnings: Vec<String>,
}

/// 活跃扫描：指令序（def/uses 对）→ 压力曲线。
/// 序列语义：第 i 点 = def 进集，若本点后不再用则出集（last-use 面）。
pub fn scan(defs: &[Option<u32>], last_uses: &[u32], p: ModelParam) -> PressureMap {
    let mut map = PressureMap { target_regs: p.target_regs, ..Default::default() };
    let mut live: Vec<u32> = Vec::new();
    for i in 0..defs.len() {
        // def 进集。
        if let Some(d) = defs[i] {
            if !live.contains(&d) {
                live.push(d);
            }
        }
        // last use 出集。
        let lu = last_uses.get(i).copied();
        if let Some(u) = lu {
            // u 是本点最后使用的寄存器 → 出集（若之后无点再引用）。
            let used_later = defs[i + 1..].iter().any(|d| *d == Some(u))
                || last_uses[i + 1..].contains(&u);
            if !used_later {
                live.retain(|x| *x != u);
            }
        }
        let regs = live.len() as u32 * p.regs_per_value;
        map.curve.push(PressurePoint { at: i, live_vals: live.len(), regs });
    }
    // 热区标注：连续超限区间。
    let mut start: Option<usize> = None;
    for pt in map.curve.iter() {
        if pt.regs > p.target_regs {
            if start.is_none() {
                start = Some(pt.at);
            }
        } else if let Some(s) = start.take() {
            map.hot_zones.push((s, pt.at - 1));
        }
    }
    if let Some(s) = start {
        let n = map.curve.len();
        map.hot_zones.push((s, n - 1));
    }
    // 预警限频合并（每热区一条——不逐点风暴）。
    for (s, e) in map.hot_zones.iter() {
        map.warnings.push(format!(
            "压力超限区间 [{},{}]（峰值 {} > 目标 {}）：建议减少同时活跃值",
            s, e, map.peak_regs(), p.target_regs
        ));
    }
    map
}

impl PressureMap {
    pub fn peak_regs(&self) -> u32 {
        self.curve.iter().map(|p| p.regs).max().unwrap_or(0)
    }
    pub fn peak_at(&self) -> usize {
        self.curve
            .iter()
            .max_by_key(|p| p.regs)
            .map(|p| p.at)
            .unwrap_or(0)
    }
}

/// 校准记录（估算与实测偏差大 → 校准账；限频一次）。
#[derive(Debug, Default)]
pub struct Calibration {
    pub records: Vec<(f64, f64)>,
    pub last_calibrated: bool,
}

impl Calibration {
    /// 校准：估算 vs 实测偏差超带宽（±25%）→ 记录并置位（限频：已置位不重录）。
    pub fn observe(&mut self, estimated: u32, measured: u32) {
        let band = 0.25;
        let est = estimated as f64;
        let mea = measured as f64;
        let deviate = (est - mea).abs() / mea.max(1.0) > band;
        if deviate && !self.last_calibrated {
            self.records.push((est, mea));
            self.last_calibrated = true;
        } else if !deviate {
            self.last_calibrated = false;
        }
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec87_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、活跃扫描。
    {
        // 序列：def r1；def r2；def r3（last use r1）；def r4（last use r2, r3）。
        let defs = vec![Some(1u32), Some(2), Some(3), Some(4)];
        let lu = vec![u32::MAX, u32::MAX, 1, 0]; // 第 2 点后 r1 不再用（标记 1）；第 3 点后 r0 哨兵
        // 用显式 last_uses：点 2 last use r1；点 3 last uses r2（哨兵面简化）。
        let lu2 = vec![0u32, 0, 1, 2];
        let p = ModelParam { regs_per_value: 1, target_regs: 8 };
        let m = scan(&defs, &lu2, p);
        // 活跃数曲线：点 0:1 → 点 1:2 → 点 2:2（r1 出）→ 点 3:2（r2 出）。
        let lives: Vec<usize> = m.curve.iter().map(|c| c.live_vals).collect();
        v.push((
            "F0487-扫描-活跃曲线",
            lives == vec![1, 2, 2, 2],
        ));
        // 参数显性：非法参数拒（零占用/零目标）。
        v.push((
            "F0487-扫描-参数守卫",
            !ModelParam { regs_per_value: 0, target_regs: 8 }.valid()
                && !ModelParam { regs_per_value: 1, target_regs: 0 }.valid()
                && ModelParam { regs_per_value: 1, target_regs: 8 }.valid(),
        ));
    }

    // 二、压力热图。
    {
        // 向量值占用 4 寄存器：3 活跃 × 4 = 12 > 目标 8 → 热区。
        let defs = vec![Some(1u32), Some(2), Some(3)];
        let lu = vec![0u32, 0, 0];
        let p = ModelParam { regs_per_value: 4, target_regs: 8 };
        let m = scan(&defs, &lu, p);
        v.push((
            "F0487-热图-峰值标注",
            m.peak_regs() == 12 && m.peak_at() == 2,
        ));
        // 热区：点 0/1 压力 ≤ 目标、点 2（12 > 8）超限 → 单区间 [2,2]。
        v.push((
            "F0487-热图-区间标注",
            m.hot_zones == vec![(2, 2)],
        ));
        // 低压力 → 零热区。
        let p2 = ModelParam { regs_per_value: 1, target_regs: 64 };
        let m2 = scan(&defs, &lu, p2);
        v.push(("F0487-热图-低压零区", m2.hot_zones.is_empty() && m2.warnings.is_empty()));
    }

    // 三、超限预警。
    {
        let defs = vec![Some(1u32), Some(2), Some(3), Some(4)];
        let lu = vec![0u32, 0, 0, 0];
        let p = ModelParam { regs_per_value: 4, target_regs: 8 };
        let m = scan(&defs, &lu, p);
        // 预警限频：每热区恰一条（不逐点风暴——4 点超限只 1 条）。
        v.push((
            "F0487-预警-限频合并",
            m.warnings.len() == m.hot_zones.len() && m.warnings.len() == 1,
        ));
        // 预警带降级建议（建议文本可机检）。
        v.push((
            "F0487-预警-降级建议",
            m.warnings.iter().all(|w| w.contains("减少同时活跃值")),
        ));
    }

    // 四、模型校准。
    {
        let mut cal = Calibration::default();
        // 偏差大（100 vs 200 → 50% > 25%）→ 记录 + 置位。
        cal.observe(100, 200);
        v.push((
            "F0487-校准-偏差记录",
            cal.records.len() == 1 && cal.last_calibrated,
        ));
        // 限频：置位期间再偏差不重录。
        cal.observe(100, 200);
        v.push(("F0487-校准-限频一次", cal.records.len() == 1));
        // 回到带宽内 → 解除置位（下轮偏差可再录）。
        cal.observe(100, 110);
        v.push(("F0487-校准-带内解除", !cal.last_calibrated && cal.records.len() == 1));
    }

    // 五、判据自检。
    {
        v.push(("F0487-判据-锚点入册", ANCHOR == "VE-F0487"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec87）。
pub fn run_vec87_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec87");
    for (name, ok) in vec87_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec87_raw_checks as run_vec87_checks;
    #[test]
    fn vec87_regpress_checks() {
        for (name, ok) in run_vec87_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
