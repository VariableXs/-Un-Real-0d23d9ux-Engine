//! F291 耗电排行 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：排行准确性（对账 F060 计量）；异常判定 3 倍阈值
//! 用例；只展示不越权判据；归因文案覆盖率（Top10 每条有说法）。
//!
//! **设计要点（主册）**：电池账本（F060）的用户面：设置中心「电源」页
//! 耗电排行 Top10（每应用：当前功率/近 1 小时累计/占比条形图），异常
//! 耗电（高于同应用历史均值 3 倍）标黄并给一句归因；排行只展示不直接
//! 杀进程（点进去给调节建议，杀不杀用户定）。
//!
//! 实装：排行计算器（功率注入——对账 F060 的注入口）；3 倍异常判定
//! （同应用历史均值 × 3——判据定值）；归因文案表（Top10 覆盖——每条
//! 有说法）；只展示纪律（输出只有建议——无杀进程动作可调用）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 异常判定倍数（主册定值）。
pub const ANOMALY_X: f64 = 3.0;
/// 排行深度。
pub const TOP_N: usize = 10;

/// 一个应用的耗电计量。
#[derive(Clone, Debug)]
pub struct PowerUsage {
    pub app: String,
    /// 当前功率（mW，F060 账本注入）。
    pub now_mw: u64,
    /// 近 1 小时累计（mWh）。
    pub hour_mwh: u64,
    /// 同应用历史均值（mW——异常判定的基准）。
    pub hist_avg_mw: u64,
    /// 归因短语（Top10 覆盖判据——每条有说法）。
    pub attribution: String,
}

impl PowerUsage {
    /// 异常判定：当前功率 > 历史均值 × 3。
    pub fn anomalous(&self) -> bool {
        self.hist_avg_mw > 0 && (self.now_mw as f64) > self.hist_avg_mw as f64 * ANOMALY_X
    }
}

/// 排行条目（渲染层直读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RankRow {
    pub app: String,
    pub now_mw: u64,
    pub hour_mwh: u64,
    /// 占比千分比（0-1000——条形图直读）。
    pub permille: u64,
    /// 异常标黄。
    pub anomalous: bool,
    /// 归因（异常时给原因 + 建议；正常时给常态说法）。
    pub attribution: String,
}

/// 排行计算：按近 1 小时累计降序取 Top10。
/// 纪律：只产出展示数据——没有任何「终止进程」类动作出口（只展示不
/// 越权判据的结构保证）。
pub fn rank(usages: &[PowerUsage]) -> Vec<RankRow> {
    let total: u64 = usages.iter().map(|u| u.hour_mwh).sum();
    let mut sorted: Vec<&PowerUsage> = usages.iter().collect();
    sorted.sort_by(|a, b| b.hour_mwh.cmp(&a.hour_mwh));
    sorted
        .into_iter()
        .take(TOP_N)
        .map(|u| {
            let permille = if total == 0 {
                0
            } else {
                u.hour_mwh * 1000 / total
            };
            let attribution = if u.anomalous() {
                if u.attribution.is_empty() {
                    String::from("耗电明显高于平时——可在应用内检查后台活动")
                } else {
                    u.attribution.clone()
                }
            } else if u.attribution.is_empty() {
                String::from("耗电正常")
            } else {
                u.attribution.clone()
            };
            RankRow {
                app: u.app.clone(),
                now_mw: u.now_mw,
                hour_mwh: u.hour_mwh,
                permille,
                anomalous: u.anomalous(),
                attribution,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_powerank_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F291");
    // 排行准确性：按累计降序 + 占比千分比（总量 2000 → 600/200/200）。
    let data = alloc::vec![
        PowerUsage { app: String::from("浏览器"), now_mw: 4_000, hour_mwh: 400, hist_avg_mw: 3_800, attribution: String::new() },
        PowerUsage { app: String::from("游戏"), now_mw: 12_000, hour_mwh: 1_200, hist_avg_mw: 11_000, attribution: String::new() },
        PowerUsage { app: String::from("记事本"), now_mw: 200, hour_mwh: 400, hist_avg_mw: 210, attribution: String::new() },
    ];
    let rows = rank(&data);
    set.add(
        "F291 rank order",
        rows[0].app == "游戏" && rows[0].permille == 600 && rows[2].permille == 200,
        "desc + permille",
    );
    // 异常判定 3 倍阈值：均值 100、当前 300 → 不超（300 == 3×100 不算
    // 严格大于）；301 → 异常。
    let edge = PowerUsage { app: String::from("A"), now_mw: 301, hour_mwh: 1, hist_avg_mw: 100, attribution: String::new() };
    let edge_eq = PowerUsage { app: String::from("B"), now_mw: 300, hour_mwh: 1, hist_avg_mw: 100, attribution: String::new() };
    set.add(
        "F291 3x threshold",
        edge.anomalous() && !edge_eq.anomalous(),
        ">3x strict",
    );
    // 异常标黄 + 归因覆盖（Top 每条有说法）。
    let anom = alloc::vec![
        PowerUsage { app: String::from("同步盘"), now_mw: 5_000, hour_mwh: 500, hist_avg_mw: 1_000, attribution: String::from("后台持续联网——可在应用内关闭同步") },
        PowerUsage { app: String::from("计算器"), now_mw: 100, hour_mwh: 10, hist_avg_mw: 95, attribution: String::new() },
    ];
    let rows2 = rank(&anom);
    set.add(
        "F291 flag+attribution",
        rows2[0].anomalous && rows2[0].attribution.contains("同步")
            && !rows2[1].anomalous && rows2[1].attribution == "耗电正常",
        "Top covered",
    );
    // 只展示不越权：RankRow 无任何进程控制字段（类型即证明）+ 建议出口。
    set.add(
        "F291 display only",
        rows2.iter().all(|r| !r.attribution.is_empty()),
        "advice not kill",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f291_rank_green() {
        let set = run_powerank_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F291 自检红 {f}/{p}");
    }

    #[test]
    fn top_n_capped() {
        let data: Vec<PowerUsage> = (0..30)
            .map(|i| PowerUsage {
                app: alloc::format!("应用{}", i),
                now_mw: i as u64,
                hour_mwh: i as u64,
                hist_avg_mw: 1,
                attribution: String::new(),
            })
            .collect();
        assert_eq!(rank(&data).len(), TOP_N, "排行只出 Top10");
    }
}
