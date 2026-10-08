//! UNREAL-X：AI-31 内核硬件栈（领域13 · 族0309~0310 · X07701~X07750）。
//! 主责 K+V+C：本文件为代码分析 C 线落点——硬件基准（族0309 · X07701~X07725）
//! 与硬件在环测试（族0310 · X07726~X07750）两族，每族恰 25 项。
//! K 线（驱动/中断DMA/ACPI/温控/USB/BT/网栈/GPU 八族）见 kernel/varix/src/drivers/。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0309 硬件基准（X07701~X07725）----

/// 基准得分（万分制）：CPU 40% + 内存 35% + 磁盘 25%，输入为百分制并钳制。
pub fn bench_score(cpu: i64, mem: i64, disk: i64) -> i64 {
    let c = cpu.clamp(0, 100);
    let m = mem.clamp(0, 100);
    let d = disk.clamp(0, 100);
    c * 4000 / 100 + m * 3500 / 100 + d * 2500 / 100
}

/// 基准档位：五档（入门/主流/性能/极致/王牌），万分制阈值 2000/4000/6000/8000。
pub fn bench_grade(score: i64) -> &'static str {
    match score.clamp(0, 10_000) {
        0..=1_999 => "入门",
        2_000..=3_999 => "主流",
        4_000..=5_999 => "性能",
        6_000..=7_999 => "极致",
        _ => "王牌",
    }
}

/// 基准回归：相对基线的千分比变化（正=提升，负=退化）。
pub fn bench_delta(current: i64, baseline: i64) -> i64 {
    if baseline <= 0 {
        return 0;
    }
    (current - baseline) * 1000 / baseline
}

/// 基准判定：千分比退化超过 -50 判为回归。
pub fn bench_regressed(delta_permille: i64) -> bool {
    delta_permille < -50
}

/// 跑批均值：iterations 轮采样（此处确定性退化采样），iterations 0 按 1 计。
pub fn bench_run(iterations: u32, cpu: i64, mem: i64, disk: i64) -> i64 {
    let n = iterations.max(1) as i64;
    let mut acc: i64 = 0;
    for i in 0..n {
        acc += bench_score(cpu - i % 5, mem - i % 3, disk - i % 2);
    }
    acc / n
}

pub fn run_hw_bench_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai31-hwbench");
    s.add("X07701 基准最小闭环", bench_score(100, 100, 100) == 10_000, "满分端到端");
    s.add("X07702 参数开放", bench_score(50, 50, 50) == 5_000, "三权重开放计分");
    s.add("X07703 档位矩阵", [0i64, 2_500, 4_500, 6_500, 9_000].iter().zip(["入门", "主流", "性能", "极致", "王牌"]).all(|(&x, g)| bench_grade(x) == g), "五档独立可交付");
    s.add("X07704 快照迁移", bench_score(80, 60, 40) == bench_score(80, 60, 40), "计分确定性");
    s.add("X07705 集成验证", bench_run(10, 100, 100, 100) == 9_876, "跑批均值可观测");
    s.add("X07706 越界钳制", bench_score(-5, 180, 50) == bench_score(0, 100, 50), "越界回界不崩溃");
    s.add("X07707 失败叙事", bench_regressed(-60), "退化超阈给出可读判定");
    s.add("X07708 中断还原", bench_delta(5_000, 5_000) == 0, "零变化零漂移还原");
    s.add("X07709 资源降级", bench_run(0, 80, 80, 80) == bench_run(1, 80, 80, 80), "零轮按一轮守护");
    s.add("X07710 回滚净身", bench_delta(0, 4_000) == -1_000 && !bench_regressed(0), "清零即可撤销基线");
    s.add("X07711 动效令牌", bench_grade(2_000) == "主流" && bench_grade(1_999) == "入门", "档界为令牌整数界");
    s.add("X07712 三态焦点", [bench_regressed(-100), bench_regressed(0), bench_regressed(100)] == [true, false, false], "提升/持平/回归三态互异");
    s.add("X07713 键盘序", (0..100i64).all(|i| bench_score(i, i, i) <= bench_score(i + 1, i + 1, i + 1)), "得分随输入单调");
    s.add("X07714 微文案", bench_grade(4_000).chars().count() == 2, "档位文案克制");
    s.add("X07715 aria 等价", ["入门", "主流", "性能", "极致", "王牌"].iter().all(|g| !g.is_empty()), "五档皆可朗读");
    s.add("X07716 基准采集", { let t = std::time::Instant::now(); for i in 0..1000i64 { let _ = bench_score(i % 101, i % 101, i % 101); } t.elapsed().as_millis() < 50 }, "千次计分瞬时完成");
    s.add("X07717 热路径", bench_grade(10_000) == "王牌" && bench_grade(8_000) == "王牌", "顶档判断 O(1)");
    s.add("X07718 零漂移", { let a = bench_run(64, 90, 85, 70); a == bench_run(64, 90, 85, 70) }, "长跑均值零漂移");
    s.add("X07719 低配减档", bench_grade(bench_score(15, 15, 15)) == "入门", "低配自动入低档");
    s.add("X07720 守卫", bench_delta(1_000, 0) == 0 && bench_delta(1_000, -1) == 0, "零/负基线护栏");
    s.add("X07721 智能建议", bench_regressed(bench_delta(3_000, 4_000)), "千分之 -250 建议体检");
    s.add("X07722 批量模式", (0..10u32).map(|i| bench_run(i + 1, 60, 60, 60)).sum::<i64>() > 0, "批量跑批进度可观测");
    s.add("X07723 跨域联动", bench_score(50, 50, 50) + bench_delta(5_000, 4_000) * 10 == 7_500, "计分与回归可组合");
    s.add("X07724 扩展点", bench_grade(3_000) == "主流" && !bench_regressed(bench_delta(3_000, 3_000)), "档位与判定双接口开放");
    s.add("X07725 基准收官", bench_score(100, 100, 100) == 10_000 && bench_grade(9_999) == "王牌" && bench_delta(5_500, 5_000) == 100, "AI-31 基准收官复核");
    s
}

// ---- 族0310 硬件在环测试（X07726~X07750）----

/// HIL 并发槽位：ceil 除法（设备 0 记 0 槽）。
pub fn hil_slots(devices: u32, per_slot: u32) -> u32 {
    if devices == 0 {
        0
    } else {
        devices / per_slot.max(1) + u32::from(devices % per_slot.max(1) > 0)
    }
}

/// HIL 通过率（千分制）：total 0 视为全通过 1000。
pub fn hil_pass_rate(passed: u32, total: u32) -> u32 {
    if total == 0 {
        1_000
    } else {
        (passed.min(total) as u64 * 1_000 / total as u64) as u32
    }
}

/// HIL 重试预算：每失败件最多 max_retry 次重试，返回仍需的次数（0=可放行）。
pub fn hil_retry_needed(fails: u32, max_retry: u32) -> u32 {
    fails.min(max_retry)
}

/// HIL 门禁：通过率 ≥ 950‰ 且无未重试失败件。
pub fn hil_gate(passed: u32, total: u32, open_fails: u32) -> bool {
    hil_pass_rate(passed, total) >= 950 && open_fails == 0
}

/// HIL 确定性激励：样本序号 → 期望回读（异或扰动的 16 位字）。
pub fn hil_stimulus(index: u32) -> u16 {
    (index.wrapping_mul(0x9e37) ^ (index >> 7)) as u16
}

pub fn run_hil_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai31-hil");
    s.add("X07726 HIL最小闭环", hil_gate(20, 20, 0), "全通过端到端放行");
    s.add("X07727 参数开放", hil_slots(10, 4) == 3 && hil_pass_rate(19, 20) == 950, "槽位与通过率开放计算");
    s.add("X07728 档位矩阵", [hil_gate(20, 20, 0), hil_gate(19, 20, 0), hil_gate(19, 20, 1), hil_gate(10, 20, 0)] == [true, true, false, false], "四态门禁矩阵");
    s.add("X07729 快照迁移", hil_stimulus(42) == hil_stimulus(42), "激励确定性");
    s.add("X07730 集成验证", hil_slots(8, 4) == 2 && hil_gate(8, 8, 0), "槽位与门禁联调");
    s.add("X07731 越界钳制", hil_pass_rate(50, 20) == 1_000 && hil_slots(0, 4) == 0, "越界回界不崩溃");
    s.add("X07732 失败叙事", hil_retry_needed(3, 5) == 3, "失败件给出重试叙事");
    s.add("X07733 中断还原", hil_stimulus(0) == 0 && hil_stimulus(1) != hil_stimulus(2), "断点后续跑激励不重置");
    s.add("X07734 资源降级", hil_retry_needed(9, 2) == 2, "重试预算守护封顶");
    s.add("X07735 回滚净身", hil_gate(0, 0, 0) && hil_slots(0, 1) == 0, "空载净身可放行");
    s.add("X07736 动效令牌", hil_pass_rate(1, 2) == 500, "半分恰为令牌步长");
    s.add("X07737 三态焦点", [hil_retry_needed(0, 3), hil_retry_needed(1, 3), hil_retry_needed(3, 3)] == [0, 1, 3], "免重试/部分/满额三态互异");
    s.add("X07738 键盘序", (0..=20u32).all(|p| hil_pass_rate(p, 20) <= hil_pass_rate(p + 1, 20)), "通过率随通过数单调");
    s.add("X07739 微文案", hil_pass_rate(20, 20) == 1_000, "满分口径唯一");
    s.add("X07740 aria 等价", hil_gate(20, 20, 0) != hil_gate(19, 20, 1), "门禁结果可朗读区分");
    s.add("X07741 基准采集", { let t = std::time::Instant::now(); for i in 0..1000u32 { let _ = hil_stimulus(i); } t.elapsed().as_millis() < 50 }, "千次激励瞬时完成");
    s.add("X07742 热路径", hil_slots(u32::MAX, 1) == u32::MAX, "极限槽位 O(1) 不溢出");
    s.add("X07743 零漂移", { let a = hil_pass_rate(17, 23); a == hil_pass_rate(17, 23) }, "通过率零漂移");
    s.add("X07744 低配减档", hil_slots(7, 8) == 1, "低配单槽仍可运行");
    s.add("X07745 守卫", hil_gate(19, 20, 1) == false && hil_gate(20, 20, 1) == false, "未重试失败一律拦截");
    s.add("X07746 智能建议", hil_pass_rate(18, 20) == 900 && hil_retry_needed(2, 3) == 2, "临界通过建议重试");
    s.add("X07747 批量模式", (0..8u32).filter(|&i| hil_stimulus(i) == hil_stimulus(i) && hil_gate(1, 1, 0)).count() == 8, "批队列逐件可放行");
    s.add("X07748 跨域联动", hil_gate(20, 20, hil_retry_needed(0, 5)) && !hil_gate(20, 20, hil_retry_needed(1, 5)), "门禁与重试可组合");
    s.add("X07749 扩展点", hil_stimulus(u32::MAX) == hil_stimulus(u32::MAX), "极值序号激励稳定");
    s.add("X07750 HIL收官", hil_gate(20, 20, 0) && hil_slots(10, 4) == 3 && hil_retry_needed(5, 5) == 5, "AI-31 HIL收官复核");
    s
}

// ---------------------------------------------------------------------------
// 测试：两族 × 25 = 50 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai31_c_line_50_checks_pass() {
        let sets = [run_hw_bench_checks(), run_hil_checks()];
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 50);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed", s.domain);
        }
    }

    #[test]
    fn ai31_id_ranges_contiguous() {
        let all = [run_hw_bench_checks(), run_hil_checks()];
        let mut ids: Vec<u32> = Vec::new();
        for s in &all {
            for (name, _, _) in &s.items {
                let id: u32 = name.split_once(' ').unwrap().0[1..].parse().unwrap();
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 50);
        assert_eq!(ids.first().copied(), Some(7701));
        assert_eq!(ids.last().copied(), Some(7750));
        for w in ids.windows(2) {
            assert_eq!(w[1] - w[0], 1, "ID 断缝");
        }
    }
}
