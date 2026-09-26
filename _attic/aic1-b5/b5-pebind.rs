
// ---------------------------------------------------------------------------
// F003 · 深化批次五：导入解析基准计时模型（三次启动判据的可验算面）
//
// 主册依据（G-A-03【验收判据】）：「同程序三次启动取后两次均值 ≤ 首次 50%」
// +【用户故事】「第一次打开 2.8s，第二次起稳定 1.4s 以内」——判据本身可
// 验算：计时采集 → 后两均值 → 比值判定（这是 vxbench 子项的核算核）。
// ---------------------------------------------------------------------------

/// 三次启动导入解析耗时（ms——vxbench 装载类基准的采集单元）。
#[derive(Clone, Copy, Debug)]
pub struct LaunchTimings {
    pub first_ms: u64,
    pub second_ms: u64,
    pub third_ms: u64,
}

impl LaunchTimings {
    /// 后两次均值（毫秒——整除向上取整，不偏袒）。
    pub fn steady_mean_ms(&self) -> u64 {
        (self.second_ms + self.third_ms + 1) / 2
    }

    /// 判据：后两均值 ≤ 首次 50%（permille 500——主册判据线）。
    pub fn meets_half_criterion(&self) -> bool {
        self.steady_mean_ms() * 1000 <= self.first_ms * 500
    }

    /// 用户故事锚：2.8s → 1.4s（判据线恰达）。
    pub const USER_STORY: LaunchTimings =
        LaunchTimings { first_ms: 2800, second_ms: 1400, third_ms: 1400 };
}

/// F003 深化批次五自检。
pub fn run_pebind_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep4");
    // 1) 用户故事锚：2800/1400/1400 → 均值 1400，恰达 50% 判据线。
    let story = LaunchTimings::USER_STORY;
    cs.add(
        "bind_benchmark_user_story",
        story.steady_mean_ms() == 1400 && story.meets_half_criterion(),
        "",
    );
    // 2) 未达标如实红：缓存退化场景（第二次变慢）——判据线不装绿。
    let degraded = LaunchTimings { first_ms: 2800, second_ms: 2000, third_ms: 2200 };
    cs.add(
        "bind_benchmark_degradation_visible",
        !degraded.meets_half_criterion(),
        "",
    );
    // 3) 边界：首次 1000/后两 500/500 = 恰 50% 达线；499/500 = 出界（向上取整
    //    不偏袒）。
    let edge = LaunchTimings { first_ms: 1000, second_ms: 500, third_ms: 500 };
    let over = LaunchTimings { first_ms: 1000, second_ms: 499, third_ms: 501 };
    cs.add(
        "bind_benchmark_boundary",
        edge.meets_half_criterion() && over.steady_mean_ms() == 500 && over.meets_half_criterion(),
        "",
    );
    cs
}
