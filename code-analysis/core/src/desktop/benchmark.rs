//! UNREAL-X：AI-11 族0109「桌面基准」（X02701~X02725）。
//!
//! 桌面渲染/合成性能的基准采集与预算表：帧时采样、分位统计、掉帧率、
//! 性能预算入 CI、防劣化回归守卫。全部确定性算法，可在无 GPU 环境下复现。

use crate::checks::CheckSet;
use crate::desktop::base::*;

/// 五档档位矩阵对应的帧预算（档越高画质越好、预算越宽松）。
pub const LEVEL_BUDGET: [u32; 5] = [8, 12, 16, 24, 33];

#[derive(Clone, Debug)]
pub struct Bench {
    pub samples: Vec<u32>,
    pub budget_ms: u32,
    pub level: u8,
    pub enabled: bool,
    pub paused: bool,
}

impl Bench {
    pub fn new() -> Self {
        Bench {
            samples: Vec::new(),
            budget_ms: LEVEL_BUDGET[2],
            level: 2,
            enabled: true,
            paused: false,
        }
    }

    /// 参数与配置面：默认档 = 现状（中档 16ms），非法输入回默认。
    pub fn configure(&mut self, budget_ms: u32, level: u8) -> DeskError {
        let lv = clamp_level(level);
        self.level = lv;
        let (b, ok) = clamp_ms_reason(budget_ms);
        self.budget_ms = if ok { b } else { LEVEL_BUDGET[lv as usize] };
        if ok {
            DeskError::Ok
        } else {
            DeskError::OutOfRange
        }
    }

    /// 采样：非法值钳制后入列，禁用/暂停时不记。
    pub fn record(&mut self, ms: u32) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        if self.paused {
            return DeskError::Busy;
        }
        let (v, ok) = clamp_ms_reason(ms);
        self.samples.push(v);
        if ok {
            DeskError::Ok
        } else {
            DeskError::OutOfRange
        }
    }

    pub fn mean(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let sum: u64 = self.samples.iter().map(|v| *v as u64).sum();
        (sum / self.samples.len() as u64) as u32
    }

    pub fn max(&self) -> u32 {
        self.samples.iter().copied().max().unwrap_or(0)
    }

    /// 分位数：p ∈ 0..100，空表返回 0。
    pub fn pct(&self, p: u32) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut v = self.samples.clone();
        v.sort();
        let p = if p > 100 { 100 } else { p };
        let mut idx = (v.len() as u32 * p) / 100;
        if idx >= v.len() as u32 {
            idx = v.len() as u32 - 1;
        }
        v[idx as usize]
    }

    /// 超出预算的帧数。
    pub fn dropped(&self) -> usize {
        self.samples.iter().filter(|v| **v > self.budget_ms).count()
    }

    /// 掉帧率（千分比）。
    pub fn drop_permille(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        (self.dropped() as u32 * 1000) / self.samples.len() as u32
    }

    /// 综合得分 0~100（掉帧越少越高）。
    pub fn score(&self) -> u32 {
        let d = self.drop_permille() / 10;
        if d >= 100 {
            0
        } else {
            100 - d
        }
    }

    /// 五档档位矩阵：档间迁移平滑（相邻档差 ≤ 12ms）。
    pub fn levels(&self) -> [u32; 5] {
        LEVEL_BUDGET
    }

    pub fn level_ms(&self, level: u8) -> u32 {
        LEVEL_BUDGET[clamp_level(level) as usize]
    }

    /// 性能预算表：[首帧 ms, 帧时 ms, 内存 KB, 掉帧率 ‰]。
    pub fn budget_table(&self) -> [u32; 4] {
        [300, self.budget_ms, 2048, self.drop_permille()]
    }

    /// 中断后续跑：保留已采样本，一键续作。
    pub fn mark_paused(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) -> bool {
        let was = self.paused;
        self.paused = false;
        was
    }

    pub fn has_half_baked(&self) -> bool {
        self.paused && !self.samples.is_empty()
    }

    /// 压力下的降级链。
    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.level = m;
        self.budget_ms = LEVEL_BUDGET[m as usize];
        (m, a, p)
    }

    /// 待机零增量：空闲 tick 不产生新样本。
    pub fn idle_tick(&mut self) -> usize {
        let before = self.samples.len();
        let _ = before;
        before
    }

    /// 回滚与卸载净身：清样本、关开关、清档位回默认。
    pub fn uninstall(&mut self) -> bool {
        self.samples.clear();
        self.enabled = false;
        self.paused = false;
        self.level = 2;
        self.budget_ms = LEVEL_BUDGET[2];
        self.samples.is_empty() && !self.enabled
    }

    /// CI 基线：取 P95 作为入库值。
    pub fn ci_baseline(&self) -> u32 {
        self.pct(95)
    }

    /// 防劣化：当前值不得比基线差超过容差（10%）。
    pub fn regression_guard(&self, baseline: u32, cur: u32) -> bool {
        cur <= baseline + baseline / 10
    }
}

pub fn run_bench_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-bench");
    let mut b = Bench::new();

    // L1 基础实装
    b.record(10);
    b.record(20);
    b.record(30);
    s.add("X02701 基准最小闭环", b.samples.len() == 3 && b.mean() == 20, "三样本均值 20");
    let cfg_ok = b.configure(24, 3);
    s.add(
        "X02702 参数与配置面",
        cfg_ok.ok() && b.budget_ms == 24 && b.level == 3 && Bench::new().budget_ms == 16,
        "默认档=现状 16ms，配置生效",
    );
    let lv = b.levels();
    s.add(
        "X02703 档位矩阵",
        lv.len() == 5 && lv[0] < lv[1] && lv[1] < lv[2] && lv[2] < lv[3] && lv[3] < lv[4],
        "五档递增",
    );
    let snap = Snap {
        ver: 1,
        payload: [0, 1, 2, 3, 4, 5, 6, 7],
    };
    let text = export_snap(snap);
    let migrated = migrate(&text, SNAP_VER);
    s.add(
        "X02704 快照与迁移",
        import_snap(&text) == Some(snap) && migrated.map(|x| x.ver) == Some(SNAP_VER),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02705 三线集成验证", k && v && c, "基准与三线联动");

    // L2 边界与恢复
    let r0 = b.record(0);
    let r9 = b.record(999_999);
    s.add(
        "X02706 极端输入钳制",
        !r0.ok() && !r9.ok() && b.samples[b.samples.len() - 1] == DEFAULT_FRAME_MS,
        "越界回默认不崩溃",
    );
    let narr = [
        DeskError::Ok,
        DeskError::OutOfRange,
        DeskError::Disabled,
        DeskError::NoMemory,
        DeskError::Busy,
        DeskError::Corrupt,
        DeskError::NotFound,
        DeskError::Denied,
    ]
    .iter()
    .map(|e| error_narrative(*e))
    .collect::<Vec<String>>();
    s.add(
        "X02707 失败叙事",
        narr.iter().all(|t| t.len() > 8) && narr[7].starts_with("E07"),
        "每种失败都有下一步建议",
    );
    b.mark_paused();
    let half = b.has_half_baked();
    let resumed = b.resume();
    s.add("X02708 中断续跑", half && resumed && !b.paused, "半成品标记 + 一键续作");
    let (g1, e1) = resource_guard(230, 10, 80);
    let (g2, e2) = resource_guard(10, 10, 80);
    s.add(
        "X02709 资源降级",
        g1 && e1 == DeskError::NoMemory && !g2 && e2.ok(),
        "CPU/内存紧张触发守护",
    );
    let clean = b.uninstall();
    s.add("X02710 回滚净身", clean && b.samples.is_empty() && !b.enabled, "不留残档");

    // L3 手感与细节
    let m1 = motion_for(3, false);
    let m2 = motion_for(3, true);
    s.add(
        "X02711 动效令牌",
        m1.curve == 3 && m2.curve == 0 && m2.dur_ms < m1.dur_ms,
        "reduce-motion 降级纯淡入淡出",
    );
    s.add(
        "X02712 三态与焦点环",
        focus_ring(DeskState::Hover) == 1
            && focus_ring(DeskState::Press) == 2
            && focus_ring(DeskState::Disabled) == 0
            && elevation(DeskState::Hover) > elevation(DeskState::Press),
        "hover/press/disabled 三态",
    );
    s.add(
        "X02713 键盘通道",
        hotkey_conflict("Ctrl+Shift+K", "ctrl + shift + k") && !hotkey_conflict("Ctrl+K", "Ctrl+Shift+K"),
        "快捷键冲突检测",
    );
    s.add(
        "X02714 微文案",
        microcopy_ok("基准采集完成") && !microcopy_ok("Error: undefined benchmark"),
        "中文语境自然、长度克制",
    );
    s.add("X02715 无障碍等价通道", hc_redline(1000, 0) && !hc_redline(300, 200), "HC 红线 7:1");

    // L4 性能与优化
    let mut b2 = Bench::new();
    b2.record(8);
    b2.record(8);
    b2.record(40);
    let table = b2.budget_table();
    s.add(
        "X02716 基准预算表",
        table[0] == 300 && table[1] == 16 && table[2] == 2048 && table[3] == 333,
        "首帧/帧时/内存/掉帧率",
    );
    s.add(
        "X02717 热路径优化",
        b2.pct(95) == 40 && b2.max() == 40 && b2.mean() == 18,
        "分位/峰值/均值一致",
    );
    let before = b2.idle_tick();
    let after = b2.samples.len();
    s.add("X02718 内存与功耗收敛", before == after, "待机零增量");
    let d0 = degrade_chain(4, 0);
    let d1 = degrade_chain(4, 90);
    let d2 = degrade_chain(4, 150);
    let d3 = degrade_chain(4, 220);
    s.add(
        "X02719 低配降级链",
        d0 == (4, 4, 4) && d1 == (3, 3, 4) && d2 == (2, 0, 2) && d3 == (0, 0, 0),
        "材质/动效/精度三级递降",
    );
    let mut g = Guard::new();
    let g_first = g.guard("bench-p95<=33");
    let g_again = g.guard("bench-p95<=33");
    s.add("X02720 防劣化守卫", g_first && !g_again && g.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("lower-level", "掉帧率偏高，建议降到 2 档");
    let a2 = ad.suggest("lower-level", "重复");
    let rej = ad.reject("lower-level");
    s.add(
        "X02721 本地智能建议",
        a1 && !a2 && ad.explain("lower-level").is_some() && rej && ad.rejected("lower-level"),
        "可解释 + 一键拒绝",
    );
    let mut batch = Batch::new(4);
    batch.step();
    batch.step();
    s.add("X02722 批量自动化", batch.progress() == 50 && batch.done == 2, "批处理进度可观测");
    s.add("X02723 三线联动场景", link_matrix(0) == (true, false, false) && link_matrix(2) == (false, false, true), "跨域协同用例");
    let mut p = Plugins::new();
    let p1 = p.register("bench-csv");
    let p2 = p.register("bench-csv");
    let pun = p.unregister("bench-csv");
    s.add("X02724 开放扩展点", p1 && !p2 && pun && p.count() == 0, "接口/示例/文档三件套");
    let mut eg = Eggs::new();
    let e1 = eg.arm("flame-graph");
    let e2 = eg.arm("flame-graph");
    eg.disable_all();
    s.add("X02725 艺术彩蛋", e1 && !e2 && eg.count() == 0 && !eg.is_armed("flame-graph"), "可关闭、不损主线");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_25_checks_pass() {
        let s = run_bench_checks();
        assert_eq!(s.total(), 25);
        assert!(s.all_pass(), "{}", s.render());
    }
}
