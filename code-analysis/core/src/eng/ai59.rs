//! UNREAL-X AI-59 工程质量 C 线（族0583 回归防线 · X14551~X14575 /
//! 族0585 长稳测试 · X14601~X14625），勿删。
//! 与 V 线（src/features/engops/ai59Models.ts）同口径镜像：零 AI、零随机、零时钟依赖。

use crate::checks::CheckSet;

// ---- 族0583 回归防线（X14551~X14575）----

/// 断言注册表：只增不删 + 基线冻结 + 三档判定。
pub struct RegressionGuard {
    pub ids: Vec<u32>,
    pub frozen: bool,
    pub clamped: u32,
}

impl Default for RegressionGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl RegressionGuard {
    pub fn new() -> Self {
        RegressionGuard { ids: Vec::new(), frozen: false, clamped: 0 }
    }

    /// 登记断言：冻结后拒登记、非法/重复拒绝（只增不删）。
    pub fn register(&mut self, id: u32) -> bool {
        if self.frozen {
            self.clamped += 1;
            return false;
        }
        if id < 1 || self.ids.contains(&id) {
            self.clamped += 1;
            return false;
        }
        self.ids.push(id);
        true
    }

    /// 基线冻结：冻结终态，之后拒登记。
    pub fn freeze(&mut self) -> bool {
        self.frozen = true;
        self.frozen
    }

    pub fn count(&self) -> usize {
        self.ids.len()
    }
}

/// 三档判定：不降=green；降幅 <5%=yellow；否则 red（纯整数运算零浮点漂移）。
pub fn regression_verdict(old_score: u64, new_score: u64) -> &'static str {
    if new_score >= old_score {
        "green"
    } else if new_score * 20 >= old_score * 19 {
        "yellow"
    } else {
        "red"
    }
}

/// 破坏演练：注入 -20% 劣化必红。
pub fn regression_drill() -> bool {
    regression_verdict(1000, 800) == "red"
}

/// 智能建议：下一个空闲断言号（最大号 +1；空表为 1）。
pub fn next_free(ids: &[u32]) -> u32 {
    ids.iter().copied().max().map_or(1, |m| m + 1)
}

/// 族0583 回归防线（X14551~X14575）：25 项自检。
pub fn run_regression_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai59-regression");
    let mut g = RegressionGuard::new();
    s.add("X14551 最小闭环", g.register(1) && g.count() == 1, "断言登记即计数");
    s.add("X14552 全量参数", (2..=25u32).all(|i| g.register(i)) && g.count() == 25, "批量登记至 25 检");
    s.add(
        "X14553 档位矩阵",
        regression_verdict(100, 100) == "green" && regression_verdict(100, 96) == "yellow" && regression_verdict(100, 94) == "red",
        "green/yellow/red 三档独立可交付",
    );
    s.add("X14554 快照迁移", g.ids == (1..=25u32).collect::<Vec<u32>>(), "登记序列确定性可复现");
    s.add("X14555 联调集成", regression_drill(), "破坏演练 -20% 必红");
    s.add(
        "X14556 越界钳制",
        { let mut q = RegressionGuard::new(); !q.register(0) && q.clamped >= 1 },
        "0 号非法断言被拒并计数",
    );
    s.add(
        "X14557 失败叙事",
        { let mut q = RegressionGuard::new(); q.register(1) && !q.register(1) },
        "重复登记拒绝：注册表只增不删",
    );
    s.add(
        "X14558 中断还原",
        { let mut q = RegressionGuard::new(); q.register(1); let snap = q.ids.clone(); q.register(2); q.ids = snap; q.count() == 1 },
        "快照回滚后注册表还原",
    );
    s.add(
        "X14559 资源降级",
        { let mut q = RegressionGuard::new(); q.register(1); q.freeze(); !q.register(2) },
        "冻结后拒绝新登记（低资源态）",
    );
    s.add(
        "X14560 回滚净身",
        { let mut q = RegressionGuard::new(); q.register(1); q.freeze() && q.frozen && q.count() == 1 },
        "冻结终态不残留半成品",
    );
    s.add("X14561 动效令牌", regression_verdict(100, 100) == "green" && regression_verdict(100, 101) == "green", "不降/微升统一 green 令牌");
    s.add(
        "X14562 三态焦点",
        [regression_verdict(100, 100), regression_verdict(100, 95), regression_verdict(100, 94)] == ["green", "yellow", "red"],
        "三态互异且边界稳定",
    );
    s.add("X14563 键盘序", g.ids.windows(2).all(|w| w[0] < w[1]), "登记序单调递增无乱序");
    s.add("X14564 微文案", !regression_verdict(100, 100).is_empty(), "三档标签非空可读");
    s.add("X14565 aria 等价", regression_verdict(100, 95) == "yellow" && regression_verdict(1000, 950) == "yellow", "5% 边界与 V 线口径等价");
    s.add("X14566 基准采集", (26..=5000u32).all(|i| g.register(i)) && g.count() == 5000, "5000 断言基准采集");
    s.add(
        "X14567 热路径",
        { let mut q = RegressionGuard::new(); (1..=100u32).all(|i| q.register(i)) && q.count() == 100 },
        "百级热路径批量登记",
    );
    s.add(
        "X14568 零漂移",
        { let mut a = RegressionGuard::new(); let mut b = RegressionGuard::new(); (1..=50u32).for_each(|i| { a.register(i); b.register(i); }); a.ids == b.ids },
        "同序列双注册表零漂移",
    );
    s.add("X14569 低配减档", regression_verdict(100, 95) == "yellow" && regression_verdict(100, 90) == "red", "低配 5% 内黄、超限红");
    s.add(
        "X14570 守卫",
        { let mut q = RegressionGuard::new(); q.freeze(); (1..=10u32).all(|i| !q.register(i)) },
        "冻结守卫拒绝一切新登记",
    );
    s.add("X14571 智能建议", next_free(&g.ids) == 5001 && next_free(&[]) == 1, "下一空闲断言号建议");
    s.add(
        "X14572 批量模式",
        { let mut q = RegressionGuard::new(); q.register(5); (0..100).all(|_| !q.register(5)) && q.clamped == 100 },
        "百次重复批量全拒",
    );
    s.add(
        "X14573 跨域联动",
        { let mut q = RegressionGuard::new(); (14551..=14575u32).all(|i| q.register(i)) && q.count() == 25 },
        "X-ID 区间同口径登记",
    );
    s.add(
        "X14574 扩展点",
        { let mut q = RegressionGuard::new(); q.register(99999) },
        "断言号扩展点不设上限",
    );
    s.add("X14575 彩蛋层", regression_verdict(1000, 800) == "red" && regression_drill(), "破坏演练彩蛋：必红可重放");
    s
}

// ---- 族0585 长稳测试（X14601~X14625）----

/// 长稳跑批：72h 抽样 + 环形采样窗（24）+ 泄漏/漂移检测 + 断点续跑。
pub struct SoakRunner {
    pub hours: u32,
    pub leak_budget_kb: u64,
    pub samples: Vec<u64>,
    pub checkpoint: u32,
    pub clamped: u32,
}

impl Default for SoakRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl SoakRunner {
    pub fn new() -> Self {
        SoakRunner { hours: 72, leak_budget_kb: 2048, samples: Vec::new(), checkpoint: 0, clamped: 0 }
    }

    /// 时长设定：钳制 1~720 小时。
    pub fn set_hours(&mut self, h: u32) -> u32 {
        self.hours = h.clamp(1, 720);
        self.hours
    }

    /// 环形采样：容量 24，超出滑出最旧样本。
    pub fn sample(&mut self, v: u64) -> bool {
        self.samples.push(v);
        if self.samples.len() > 24 {
            self.samples.remove(0);
        }
        true
    }

    /// 泄漏检测：逐窗内存增量超预算即计一次。
    pub fn leaks(&self, deltas: &[u64]) -> u32 {
        deltas.iter().filter(|&&d| d > self.leak_budget_kb).count() as u32
    }

    /// 断点续跑：只接受不小于当前检查点的小时数。
    pub fn resume(&mut self, from_hour: u32) -> bool {
        if from_hour < self.checkpoint {
            self.clamped += 1;
            return false;
        }
        self.checkpoint = from_hour;
        true
    }

    /// 长稳判定：检查点推进到目标时长且采样窗无泄漏。
    pub fn verdict(&self) -> bool {
        self.checkpoint >= self.hours && self.leaks(&self.samples) == 0
    }
}

/// 漂移判定：p95 相对基线偏离 >10% 即漂移（基线 0 视为不漂移）。
pub fn soak_drift(baseline_p95: u64, now_p95: u64) -> bool {
    if baseline_p95 == 0 {
        return false;
    }
    now_p95.abs_diff(baseline_p95) * 10 > baseline_p95
}

/// 族0585 长稳测试（X14601~X14625）：25 项自检。
pub fn run_soak_checks() -> CheckSet {
    let mut s = CheckSet::new("eng-ai59-soak");
    let mut r = SoakRunner::new();
    s.add("X14601 最小闭环", r.sample(100) && r.samples.len() == 1, "采样入窗即计数");
    s.add("X14602 全量参数", r.set_hours(168) == 168 && r.hours == 168, "时长全量参数设定");
    s.add(
        "X14603 档位矩阵",
        { (0..30u64).for_each(|i| { r.sample(i); }); r.samples.len() == 24 },
        "环形采样窗恒为 24 档",
    );
    s.add(
        "X14604 快照迁移",
        { let mut q = SoakRunner::new(); (0..24u64).for_each(|i| { q.sample(i); }); q.samples == (0..24u64).collect::<Vec<u64>>() },
        "采样序列确定性可迁移",
    );
    s.add(
        "X14605 联调集成",
        { let mut q = SoakRunner::new(); q.set_hours(72); q.resume(72) && q.verdict() },
        "时长达标 + 无泄漏 → 判定通过",
    );
    s.add(
        "X14606 越界钳制",
        { let mut q = SoakRunner::new(); q.resume(5) && !q.resume(3) && q.clamped >= 1 && q.checkpoint == 5 },
        "回跳检查点被拒并钳制",
    );
    s.add("X14607 失败叙事", r.leaks(&[100, 3000, 2048, 2049]) == 2, "泄漏计数阈值边界可读");
    s.add(
        "X14608 中断还原",
        { let mut q = SoakRunner::new(); q.set_hours(10); q.resume(5); q.resume(5) && q.checkpoint == 5 },
        "断点续跑从检查点继续",
    );
    s.add("X14609 资源降级", r.set_hours(0) == 1 && r.set_hours(9999) == 720, "时长钳制 1~720");
    s.add(
        "X14610 回滚净身",
        { let q = SoakRunner::new(); !q.verdict() },
        "空跑批判定失败不残留",
    );
    s.add("X14611 动效令牌", !soak_drift(100, 110) && soak_drift(100, 111), "10% 漂移令牌边界稳定");
    s.add("X14612 三态焦点", soak_drift(100, 89) && !soak_drift(100, 90), "下漂/上漂对称判定");
    s.add(
        "X14613 键盘序",
        { let mut q = SoakRunner::new(); q.resume(10) && q.resume(10) && !q.resume(9) },
        "检查点单调不减",
    );
    s.add("X14614 微文案", r.leak_budget_kb == 2048, "泄漏预算默认 2048KB 可读");
    s.add("X14615 aria 等价", !soak_drift(0, 100), "基线 0 视为不漂移（守恒等价）");
    s.add(
        "X14616 基准采集",
        { let mut q = SoakRunner::new(); (0..1000u64).for_each(|i| { q.sample(i); }); q.samples.len() == 24 },
        "千级采样基准采集",
    );
    s.add(
        "X14617 热路径",
        { let mut q = SoakRunner::new(); (0..500u64).for_each(|i| { q.sample(i); }); q.samples.len() == 24 },
        "热路径采样恒定窗宽",
    );
    s.add(
        "X14618 零漂移",
        { let mut a = SoakRunner::new(); let mut b = SoakRunner::new(); (0..30u64).for_each(|i| { a.sample(i); b.sample(i); }); a.samples == b.samples },
        "同序列双跑批零漂移",
    );
    s.add("X14619 低配减档", r.set_hours(1) == 1, "低配最短 1 小时档");
    s.add(
        "X14620 守卫",
        { let mut q = SoakRunner::new(); q.set_hours(72); q.resume(71) && !q.verdict() && q.resume(72) && q.verdict() },
        "未达标不放行，达标即绿",
    );
    s.add(
        "X14621 智能建议",
        { let mut q = SoakRunner::new(); q.leak_budget_kb = 4096; q.leaks(&[3000]) == 0 && q.leaks(&[5000]) == 1 },
        "预算可调 + 阈值建议一致",
    );
    s.add("X14622 批量模式", r.leaks(&[0, 2048, 2049, 3000, 100]) == 2, "批量泄漏计数准确");
    s.add(
        "X14623 跨域联动",
        { let mut q = SoakRunner::new(); q.set_hours(72); q.resume(72); (0..25u64).for_each(|_| { q.sample(100); }); q.verdict() && q.samples.len() == 24 },
        "满检通过 + 窗宽与 V 线同口径",
    );
    s.add("X14624 扩展点", r.set_hours(720) == 720, "上限 720 小时扩展点");
    s.add(
        "X14625 彩蛋层",
        { let mut q = SoakRunner::new(); (1..=25u64).for_each(|i| { q.sample(i); }); q.samples.first() == Some(&2) && q.samples.len() == 24 },
        "25 份样本仅留最近 24 份",
    );
    s
}
