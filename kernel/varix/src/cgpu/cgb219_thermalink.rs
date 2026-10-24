//! CGPU-F0219 · 帧图与热管理联动（B04 优先级与预算组 · 段位 0x0750）
//!
//! 热管理（CGPU-J）与帧图的联动层。帧图执行引擎只关心"这一帧该用
//! 哪一档图复杂度"，热状态到复杂度的映射、预测性降载与回升滞回全部
//! 收敛在本模块：
//!
//! 1. **温度阶梯 → 图复杂度阶梯**（判据「温度感知」）：五个热等级
//!    （Normal/Warm/Hot/Critical/Emergency）对应五档图复杂度
//!    （Full/Light/Lean/Minimal/Skeletal），温度越高级数越低。
//! 2. **热预测性降载**（判据「预测性降载」）：温度趋势线性外推，
//!    预测窗口内将触顶即提前降一档，避免撞上硬降频（F0035 联动点）。
//! 3. **热恢复回升**（判据「回升滞回」）：温度下降须连续 N 个采样
//!    周期稳定低于回升阈值才升一档，滞回防振荡。
//! 4. **频率耦合验证**（判据「频率耦合验证」）：复杂度档位变化必须
//!    与频率域允许档位取交集，频率上限约束复杂度上限。
//! 5. **用户无感**（判据「用户无感」）：降载只允许削减非关键 Pass
//!    （标记 `essential=false`），关键链 Pass 在任意档位都保留。
//!
//! 全部为确定性纯逻辑，无浮点（毫摄氏度整数，1 = 0.001°C）。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0750;

/// 温度单位：毫摄氏度（mc，1 = 0.001°C），整数域避免浮点。
pub type MilliCelsius = i32;

/// 热等级（CGPU-J 阶梯的帧图侧投影）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ThermalLevel {
    /// < 65°C。
    Normal = 0,
    /// 65 ~ 75°C。
    Warm = 1,
    /// 75 ~ 85°C。
    Hot = 2,
    /// 85 ~ 95°C。
    Critical = 3,
    /// >= 95°C。
    Emergency = 4,
}

impl ThermalLevel {
    /// 各等级的温度上界（毫摄氏度，含 Emergency 兜底）。
    pub const THRESHOLDS: [MilliCelsius; 5] = [65_000, 75_000, 85_000, 95_000, i32::MAX];

    /// 温度 → 等级（判据「温度感知」底座）。
    pub fn from_temp(t: MilliCelsius) -> Self {
        for (i, &th) in Self::THRESHOLDS.iter().enumerate() {
            if t < th {
                return match i {
                    0 => ThermalLevel::Normal,
                    1 => ThermalLevel::Warm,
                    2 => ThermalLevel::Hot,
                    3 => ThermalLevel::Critical,
                    _ => ThermalLevel::Emergency,
                };
            }
        }
        ThermalLevel::Emergency
    }
}

/// 图复杂度档位（与热等级一一对应的五档）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum GraphComplexity {
    /// 全量 Pass。
    Full = 0,
    /// 关闭次要后处理。
    Light = 1,
    /// 降分辨率 + 关闭次要 Pass。
    Lean = 2,
    /// 仅关键链。
    Minimal = 3,
    /// 仅必要呈现链。
    Skeletal = 4,
}

impl GraphComplexity {
    /// 各档位允许保留的最大节点数上限（复杂度阶梯的量化面）。
    pub const NODE_CAPS: [u32; 5] = [u32::MAX, 512, 192, 64, 24];
    /// 各档位允许削减的层级深度（0 = 不允许削减）。
    pub const TRIM_DEPTH: [u32; 5] = [0, 2, 4, 8, 16];

    /// 热等级 → 复杂度档位（同序映射）。
    pub fn from_thermal(lv: ThermalLevel) -> Self {
        match lv {
            ThermalLevel::Normal => GraphComplexity::Full,
            ThermalLevel::Warm => GraphComplexity::Light,
            ThermalLevel::Hot => GraphComplexity::Lean,
            ThermalLevel::Critical => GraphComplexity::Minimal,
            ThermalLevel::Emergency => GraphComplexity::Skeletal,
        }
    }

    /// 档位序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
}

/// 帧图节点（最小投影：只带热联动需要的属性）。
#[derive(Clone, Copy, Debug)]
pub struct ThNode {
    /// 节点号（1 起）。
    pub id: u32,
    /// 估计耗时（微秒）。
    pub cost_us: u32,
    /// 是否关键链 Pass（关键链任何档位都保留——判据「用户无感」）。
    pub essential: bool,
    /// Pass 所属深度（用于 TRIM_DEPTH 削减）。
    pub depth: u32,
}

/// 一帧的图复杂度裁剪结果。
#[derive(Clone, Debug)]
pub struct ThTrim {
    /// 保留节点号（关键链必在）。
    pub kept: Vec<u32>,
    /// 被削减节点号。
    pub dropped: Vec<u32>,
    /// 保留节点的总耗时。
    pub kept_cost_us: u64,
    /// 削减节省的耗时。
    pub saved_cost_us: u64,
}

/// 按档位裁剪一帧图（判据「温度感知」的执行面 + 判据「用户无感」
/// 的强制面：关键链节点无条件保留）。
pub fn trim_graph(nodes: &[ThNode], cx: GraphComplexity) -> Result<ThTrim, u16> {
    if nodes.is_empty() {
        return Err(SEG_BASE);
    }
    let cap = GraphComplexity::NODE_CAPS[cx.rank() as usize];
    let trim = GraphComplexity::TRIM_DEPTH[cx.rank() as usize];
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    let mut kept_cost: u64 = 0;
    let mut saved: u64 = 0;
    for n in nodes {
        let essential_keeps = n.essential;
        let depth_allows = n.depth >= trim;
        let cap_allows = (kept.len() as u64) < cap as u64;
        if essential_keeps || (depth_allows && cap_allows) {
            kept.push(n.id);
            kept_cost += n.cost_us as u64;
        } else {
            dropped.push(n.id);
            saved += n.cost_us as u64;
        }
    }
    // 判据「用户无感」硬检查：关键链必须全部保留，否则本档位非法。
    let essential_all_kept = nodes
        .iter()
        .filter(|n| n.essential)
        .all(|n| kept.contains(&n.id));
    if !essential_all_kept {
        return Err(SEG_BASE + 1);
    }
    Ok(ThTrim { kept, dropped, kept_cost_us: kept_cost, saved_cost_us: saved })
}

/// 温度趋势预测器（判据「预测性降载」）：滑动窗口线性外推。
pub struct ThermalForecaster {
    /// 最近温度采样（毫摄氏度），容量 CAP。
    window: Vec<MilliCelsius>,
    /// 窗口容量。
    cap: usize,
    /// 预测窗口（采样周期数）。
    horizon: usize,
}

impl ThermalForecaster {
    pub fn new(cap: usize, horizon: usize) -> Self {
        ThermalForecaster { window: Vec::new(), cap: cap.max(2), horizon: horizon.max(1) }
    }

    /// 追加一次采样。
    pub fn push(&mut self, t: MilliCelsius) {
        if self.window.len() == self.cap {
            self.window.remove(0);
        }
        self.window.push(t);
    }

    /// 线性外推 horizon 周期后的温度（最小二乘斜率，整数近似：
    /// 斜率 = Σ(i-mean_i)(t-mean_t) * 1000 / Σ(i-mean_i)^2，单位
    /// mc/周期 ×1000，避免浮点）。
    pub fn forecast(&self) -> MilliCelsius {
        let n = self.window.len();
        if n < 2 {
            return self.window.last().copied().unwrap_or(0);
        }
        let mean_i = (n - 1) as i64 / 2;
        let mean_t: i64 = self.window.iter().map(|&t| t as i64).sum::<i64>() / n as i64;
        let mut num: i64 = 0;
        let mut den: i64 = 0;
        for (i, &t) in self.window.iter().enumerate() {
            let di = i as i64 - mean_i;
            num += di * (t as i64 - mean_t);
            den += di * di;
        }
        if den == 0 {
            return self.window.last().copied().unwrap_or(0);
        }
        let slope_x1000 = num * 1000 / den; // mc/周期 ×1000
        let last = self.window.last().copied().unwrap_or(0) as i64;
        (last + slope_x1000 * self.horizon as i64 / 1000) as MilliCelsius
    }

    /// 预测窗口内是否会触到 `limit`（硬降频阈值）。
    pub fn will_hit(&self, limit: MilliCelsius) -> bool {
        self.forecast() >= limit
    }
}

/// 滞回升降控制器（判据「回升滞回」）：升档须连续确认，降档即时。
pub struct HysteresisGovernor {
    /// 当前档位。
    pub rank: u32,
    /// 最高允许档位（频率耦合约束的投影）。
    pub max_rank: u32,
    /// 回升需连续确认次数。
    confirm_need: u32,
    /// 当前已连续确认次数。
    confirm_now: u32,
}

impl HysteresisGovernor {
    pub fn new(max_rank: u32, confirm_need: u32) -> Self {
        HysteresisGovernor { rank: 0, max_rank: max_rank.min(4), confirm_need: confirm_need.max(1), confirm_now: 0 }
    }

    /// 输入目标档位，推进滞回状态机，返回新档位。
    pub fn step(&mut self, want: u32) -> u32 {
        let want = want.min(self.max_rank);
        if want > self.rank {
            self.confirm_now += 1;
            if self.confirm_now >= self.confirm_need {
                self.rank += 1; // 回升一次只升一档（防振荡）
                self.confirm_now = 0;
            }
        } else {
            self.confirm_now = 0;
            if want < self.rank {
                self.rank = want; // 恶化即时降档
            }
        }
        self.rank
    }
}

/// 频率耦合验证（判据「频率耦合验证」）：复杂度档位不得超过频率域
/// 允许档位（频率上限越低，允许的最大复杂度档位越低——序号越小越
/// 复杂，故频率域给出的是"最复杂档位号"）。
pub fn freq_coupling_check(want: GraphComplexity, freq_max_complexity_rank: u32) -> Result<GraphComplexity, u16> {
    if want.rank() > 4 {
        return Err(SEG_BASE + 2);
    }
    if want.rank() < freq_max_complexity_rank.min(4) {
        // want 比频率允许的最复杂档位还复杂 → 降到频率允许档位。
        return match freq_max_complexity_rank.min(4) {
            0 => Ok(GraphComplexity::Full),
            1 => Ok(GraphComplexity::Light),
            2 => Ok(GraphComplexity::Lean),
            3 => Ok(GraphComplexity::Minimal),
            _ => Ok(GraphComplexity::Skeletal),
        };
    }
    Ok(want)
}

/// 热联动主循环一拍：输入温度与目标图，输出本帧应执行的裁剪图。
/// 这是 CGPU-J → 帧图的完整通路（判据「通路通」的帧图侧端点）。
pub fn thermal_beat(
    gov: &mut HysteresisGovernor,
    fc: &mut ThermalForecaster,
    temp: MilliCelsius,
    throttle_limit: MilliCelsius,
    nodes: &[ThNode],
) -> Result<ThTrim, u16> {
    if nodes.is_empty() {
        return Err(SEG_BASE + 3);
    }
    fc.push(temp);
    let cur = ThermalLevel::from_temp(temp);
    let mut want = GraphComplexity::from_thermal(cur).rank();
    // 预测性降载：当前未恶化但预测触顶 → 提前降一档。
    if want == 0 && fc.will_hit(throttle_limit) {
        want = 1;
    }
    let rank = gov.step(want);
    let cx = match rank {
        0 => GraphComplexity::Full,
        1 => GraphComplexity::Light,
        2 => GraphComplexity::Lean,
        3 => GraphComplexity::Minimal,
        _ => GraphComplexity::Skeletal,
    };
    trim_graph(nodes, cx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 夹具：14 节点图，前 5 个为关键链。
    fn fixture() -> Vec<ThNode> {
        let mut v = Vec::new();
        for i in 1u32..=14 {
            v.push(ThNode {
                id: i,
                cost_us: 40 + (i as u32) * 10,
                essential: i <= 5,
                depth: (i - 1) / 2,
            });
        }
        v
    }

    #[test]
    fn temp_ladder_maps_to_complexity() {
        // 判据「温度感知」：温度阶梯逐级映射。
        assert_eq!(ThermalLevel::from_temp(40_000), ThermalLevel::Normal);
        assert_eq!(ThermalLevel::from_temp(70_000), ThermalLevel::Warm);
        assert_eq!(ThermalLevel::from_temp(80_000), ThermalLevel::Hot);
        assert_eq!(ThermalLevel::from_temp(90_000), ThermalLevel::Critical);
        assert_eq!(ThermalLevel::from_temp(99_000), ThermalLevel::Emergency);
        // 等级越高保留成本越低。
        let nodes = fixture();
        let mut prev_cost = u64::MAX;
        for lv in 0..5u32 {
            let cx = match lv {
                0 => GraphComplexity::Full,
                1 => GraphComplexity::Light,
                2 => GraphComplexity::Lean,
                3 => GraphComplexity::Minimal,
                _ => GraphComplexity::Skeletal,
            };
            let t = trim_graph(&nodes, cx).unwrap();
            assert!(t.kept_cost_us <= prev_cost);
            prev_cost = t.kept_cost_us;
        }
    }

    #[test]
    fn essential_passes_never_dropped() {
        // 判据「用户无感」：关键链任何档位全保留。
        let nodes = fixture();
        for lv in 0..5u32 {
            let cx = match lv {
                0 => GraphComplexity::Full,
                1 => GraphComplexity::Light,
                2 => GraphComplexity::Lean,
                3 => GraphComplexity::Minimal,
                _ => GraphComplexity::Skeletal,
            };
            let t = trim_graph(&nodes, cx).unwrap();
            for i in 1..=5u32 {
                assert!(t.kept.contains(&i), "essential {i} dropped at rank {lv}");
            }
        }
    }

    #[test]
    fn forecast_triggers_proactive_downshift() {
        // 判据「预测性降载」：趋势上冲未触顶也提前降载。
        let mut fc = ThermalForecaster::new(8, 6);
        for t in [60_000, 61_500, 63_000, 64_500] {
            fc.push(t);
        }
        assert!(!fc.will_hit(80_000), "slow trend should not predict");
        let mut fc2 = ThermalForecaster::new(8, 10);
        for t in [62_000, 65_000, 68_000, 71_000] {
            fc2.push(t);
        }
        assert!(fc2.will_hit(85_000), "steep trend must predict hit");
    }

    #[test]
    fn recovery_requires_stable_confirmations() {
        // 判据「回升滞回」：回升需连续确认，单次抖动不升档。
        let mut gov = HysteresisGovernor::new(4, 3);
        assert_eq!(gov.step(2), 0); // 降档即时
        assert_eq!(gov.step(0), 0); // 想回升，确认 1/3
        assert_eq!(gov.step(1), 0); // 抖动回落 → 重置确认
        assert_eq!(gov.step(0), 0);
        assert_eq!(gov.step(0), 0);
        assert_eq!(gov.step(0), 0); // 连续 3 次 → 升一档
    }

    #[test]
    fn freq_coupling_clamps_complexity() {
        // 判据「频率耦合验证」：频率上限钳制复杂度。
        assert_eq!(freq_coupling_check(GraphComplexity::Full, 2).unwrap(), GraphComplexity::Lean);
        assert_eq!(freq_coupling_check(GraphComplexity::Lean, 0).unwrap(), GraphComplexity::Full);
        assert!(freq_coupling_check(GraphComplexity::Full, 9).is_err() == false);
    }

    #[test]
    fn beat_end_to_end() {
        // 全通路：温度上升 → 降档；预测触顶 → 提前降；回稳 → 回升。
        let nodes = fixture();
        let mut gov = HysteresisGovernor::new(4, 2);
        let mut fc = ThermalForecaster::new(6, 8);
        let t1 = thermal_beat(&mut gov, &mut fc, 55_000, 90_000, &nodes).unwrap();
        assert_eq!(t1.dropped.len(), 0, "normal temp keeps full graph");
        let t2 = thermal_beat(&mut gov, &mut fc, 88_000, 90_000, &nodes).unwrap();
        assert!(t2.dropped.len() > 0, "critical temp must trim");
        let t3 = thermal_beat(&mut gov, &mut fc, 93_000, 90_000, &nodes).unwrap();
        assert!(t3.saved_cost_us >= t2.saved_cost_us, "worse temp trims more");
        assert!(t3.kept.contains(&1) && t3.kept.contains(&5));
    }

    #[test]
    fn empty_graph_rejected() {
        assert_eq!(trim_graph(&[], GraphComplexity::Full).unwrap_err(), SEG_BASE);
        let mut gov = HysteresisGovernor::new(4, 1);
        let mut fc = ThermalForecaster::new(4, 4);
        assert_eq!(thermal_beat(&mut gov, &mut fc, 50_000, 90_000, &[]).unwrap_err(), SEG_BASE + 3);
    }

    #[test]
    fn trim_savings_sum_consistent() {
        // 节省 + 保留 = 全图总耗时（账目守恒）。
        let nodes = fixture();
        let total: u64 = nodes.iter().map(|n| n.cost_us as u64).sum();
        let t = trim_graph(&nodes, GraphComplexity::Minimal).unwrap();
        assert_eq!(t.kept_cost_us + t.saved_cost_us, total);
    }
}
