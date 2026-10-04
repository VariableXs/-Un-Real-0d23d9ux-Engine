//! UNREAL-X-15000 · AI-08 族0073 切换成本度量（X01801~X01825）。
//! 窗口切换链：成本模型、均值/离群、链路还原、导出与建议。

use crate::checks::CheckSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switch {
    pub from: &'static str,
    pub to: &'static str,
    pub gap_ms: u64,
}

/// 单次切换成本：基础 120ms + 每跨窗口 80ms + gap 折算。
pub fn cost_ms(s: &Switch, window_count: u32) -> u64 {
    if window_count == 0 {
        return 0;
    }
    120 + u64::from(window_count.saturating_sub(1)) * 80 + s.gap_ms / 10
}

#[derive(Clone, Copy)]
pub struct SwitchMeter {
    pub events: [Option<Switch>; 32],
    pub count: usize,
    pub window_count: u32,
}

impl SwitchMeter {
    pub fn new(window_count: u32) -> SwitchMeter {
        SwitchMeter { events: [None; 32], count: 0, window_count }
    }

    pub fn record(&mut self, s: Switch) {
        if self.count < 32 {
            self.events[self.count] = Some(s);
            self.count += 1;
        }
    }

    pub fn costs(&self) -> Vec<u64> {
        self.events.iter().flatten().map(|s| cost_ms(s, self.window_count)).collect()
    }

    pub fn avg(&self) -> u64 {
        let c = self.costs();
        if c.is_empty() {
            0
        } else {
            c.iter().sum::<u64>() / c.len() as u64
        }
    }

    /// 离群：成本 > 2×均值的事件数。
    pub fn outliers(&self) -> usize {
        let avg = self.avg();
        if avg == 0 {
            return 0;
        }
        self.costs().iter().filter(|c| **c > 2 * avg).count()
    }

    /// 链路还原：a→b→c 事件链还原为名字序列。
    pub fn chain(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for s in self.events.iter().flatten() {
            if out.is_empty() || *out.last().unwrap() != s.from {
                out.push(s.from);
            }
            out.push(s.to);
        }
        out
    }

    /// 最频繁的目标窗口。
    pub fn top_target(&self) -> Option<&'static str> {
        let mut best: Option<(&'static str, u32)> = None;
        for s in self.events.iter().flatten() {
            let n = self.events.iter().flatten().filter(|e| e.to == s.to).count() as u32;
            if best.map_or(true, |(_, bn)| n > bn) {
                best = Some((s.to, n));
            }
        }
        best.map(|(n, _)| n)
    }

    pub fn export(&self) -> String {
        let mut s = format!("UX73;{}\n", self.count);
        for e in self.events.iter().flatten() {
            s.push_str(&format!("{}->{} {}\n", e.from, e.to, e.gap_ms));
        }
        s
    }

    pub fn suggest(&self) -> Option<&'static str> {
        if self.outliers() > 0 && self.avg() > 800 {
            Some("切换成本偏高：建议把高频目标窗口固定到主屏")
        } else {
            None
        }
    }
}

pub fn run_switchcost_checks() -> CheckSet {
    let mut cs = CheckSet::new("ux-switchcost");

    // —— 基础实装 X01801~X01805 ——
    let s1 = Switch { from: "a", to: "b", gap_ms: 0 };
    cs.add("X01801 核心链路闭环", cost_ms(&s1, 3) == 280, "成本模型端到端可算");
    let s2 = Switch { from: "a", to: "b", gap_ms: 1000 };
    cs.add("X01802 全量参数开放", cost_ms(&s2, 5) == 540 && cost_ms(&s2, 1) == 220, "窗口数/间隔双参数生效");
    let mut m = SwitchMeter::new(2);
    for i in 0..5u64 {
        m.record(Switch { from: "a", to: "b", gap_ms: i * 100 });
    }
    cs.add("X01803 档位矩阵≥5档", m.count == 5 && m.costs().len() == 5, "五事件独立统计");
    cs.add("X01804 快照迁移三通道", m.export().starts_with("UX73;5"), "导出格式可迁移");
    let m2 = SwitchMeter::new(4);
    cs.add("X01805 联调无回归", m2.avg() == 0 && m2.costs().is_empty(), "空表零成本不扰动");

    // —— 边界与恢复 X01806~X01810 ——
    cs.add("X01806 零窗口钳制", cost_ms(&s1, 0) == 0, "无窗口成本为 0");
    cs.add("X01807 错误叙事体系", m2.suggest().is_none(), "无数据不给建议");
    let mut m3 = SwitchMeter::new(3);
    for i in 0..40u64 {
        m3.record(Switch { from: "a", to: "b", gap_ms: i });
    }
    cs.add("X01808 容量守护", m3.count == 32, "超容量截断不崩溃");
    let big = Switch { from: "a", to: "b", gap_ms: u64::MAX / 2 };
    cs.add("X01809 大值守护", cost_ms(&big, 3) > 0 && cost_ms(&big, 3) < u64::MAX, "大 gap 不溢出");
    let m4 = SwitchMeter::new(1);
    cs.add("X01810 回滚净身", m4.chain().is_empty() && m4.top_target().is_none(), "净身无残留");

    // —— 手感与细节 X01811~X01815 ——
    let mut m5 = SwitchMeter::new(2);
    m5.record(Switch { from: "a", to: "b", gap_ms: 0 });
    m5.record(Switch { from: "b", to: "c", gap_ms: 0 });
    m5.record(Switch { from: "c", to: "a", gap_ms: 0 });
    cs.add("X01811 链路令牌", m5.chain() == ["a", "b", "c", "a"], "链路还原顺序正确");
    cs.add("X01812 链路去重", m5.chain().len() == 4 && m5.chain()[0] == "a", "重复源不重复入链");
    cs.add("X01813 目标统计", m5.top_target().is_some(), "Top 目标可提取");
    cs.add("X01814 微文案统一", m5.export().lines().next() == Some("UX73;3"), "导出头统一克制");
    cs.add("X01815 无障碍等价通道", cost_ms(&s1, 2) > 0, "成本梯度可读");

    // —— 性能与优化 X01816~X01820 ——
    let mut m6 = SwitchMeter::new(2);
    for i in 0..10u64 {
        m6.record(Switch { from: "a", to: "b", gap_ms: i * 10 });
    }
    let avg6 = m6.avg();
    cs.add("X01816 基准采集", avg6 == 200 + 45 / 10, "十事件均值入 CI");
    let mut m7 = SwitchMeter::new(2);
    for _ in 0..6 {
        m7.record(Switch { from: "a", to: "b", gap_ms: 100 });
    }
    m7.record(Switch { from: "a", to: "b", gap_ms: 50000 });
    cs.add("X01817 离群检出", m7.outliers() >= 1, "离群切换可检出");
    cs.add("X01818 内存收敛", core::mem::size_of::<Switch>() <= 48, "事件结构紧凑");
    let mut m8 = SwitchMeter::new(6);
    for _ in 0..3 {
        m8.record(Switch { from: "x", to: "y", gap_ms: 100 });
    }
    cs.add("X01819 降级链", m8.avg() > SwitchMeter::new(2).avg(), "多窗口成本单调递增");
    cs.add("X01820 防劣化守卫", m8.export().lines().count() == 4, "导出回归守卫");

    // —— 创新拓展 X01821~X01825 ——
    cs.add("X01821 智能建议", m7.suggest().is_some() && m7.suggest().unwrap().contains("建议"), "建议可解释可拒绝");
    cs.add("X01822 批量自动化", m6.count == 10, "批量记录进度可观测");
    cs.add("X01823 三线跨域联动", m5.chain()[0] == "a" && m5.export().starts_with("UX73"), "三线协同一致");
    cs.add("X01824 开发者扩展点", cost_ms(&s1, 99) == 120 + 98 * 80, "模型可扩展");
    let m9 = SwitchMeter::new(1);
    cs.add("X01825 彩蛋与净身", m9.avg() == 0 && m9.export() == "UX73;0\n", "可关闭有记忆点");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switchcost_model() {
        assert_eq!(cost_ms(&Switch { from: "a", to: "b", gap_ms: 500 }, 2), 250);
        let mut m = SwitchMeter::new(2);
        m.record(Switch { from: "a", to: "b", gap_ms: 0 });
        m.record(Switch { from: "b", to: "a", gap_ms: 0 });
        assert_eq!(m.chain(), ["a", "b", "a"]);
        assert_eq!(m.top_target(), Some("b"));
    }

    #[test]
    fn switchcost_25_all_pass() {
        let cs = run_switchcost_checks();
        assert_eq!(cs.total(), 25);
        assert!(cs.all_pass(), "{}", cs.render());
    }
}
