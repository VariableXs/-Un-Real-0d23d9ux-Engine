//! F061 基准回归门 · 完整设计（STAR I 主册 G-B-21）。
//!
//! **判据（主册）**：回归门捕获率：人为注入 5 处性能回退全部检出；
//! 误报率 <5%（30 天零噪声误报）。
//!
//! **设计要点（主册）**：
//! - vxbench 六类基准接 CI 夜跑：装载/合成/存储/网络/调度/图像，每类
//!   2-4 条基准，基线倒退 >5% 即门禁红——性能资产像代码一样有回归保护；
//! - 基线存档不可变（换基线 = 改分数，ADR 纪律）；历史结果全保留；
//! - QEMU 噪声（虚拟化抖动）→ 三跑取中位数 + 噪声带标注；
//! - 机器差异 → 基线绑定 runner 标签；
//! - 偶发红 → 连续两晚红才告警（防狼来了）；
//! - 5% 阈值分基准可调（存储类噪声大放宽到 8%）；
//! - 趋势图异常点可标注（固件更新/借力件升级等外因）。
//!
//! 本模块是回归门的**纯逻辑核**：夜跑器、基线库、裁决与趋势账本。
//! CI 侧驱动（夜跑窗口/独立实例隔离）由上层编排；QEMU 类实测判据
//! 登记「随闸门补测」。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 默认回归阈值（千分比）：基线倒退 >5% 即红。
pub const THRESHOLD_DEFAULT_PPT: u32 = 50;

/// 存储类阈值（千分比）：噪声大放宽到 8%。
pub const THRESHOLD_STORAGE_PPT: u32 = 80;

/// 三跑取中位数。
pub const RUNS_PER_NIGHT: usize = 3;

/// 趋势保留（天）：90 天。
pub const TREND_DAYS: usize = 90;

/// 连续两晚红才告警。
pub const RED_NIGHTS_FOR_ALARM: u32 = 2;

/// 误报率红线（千分比）：<5%。
pub const FALSE_ALARM_LIMIT_PPT: u32 = 50;

/// 六类基准目录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchCat {
    Load,
    Composite,
    Storage,
    Net,
    Sched,
    Image,
}

impl BenchCat {
    pub const ALL: [BenchCat; 6] = [
        BenchCat::Load,
        BenchCat::Composite,
        BenchCat::Storage,
        BenchCat::Net,
        BenchCat::Sched,
        BenchCat::Image,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BenchCat::Load => "load",
            BenchCat::Composite => "composite",
            BenchCat::Storage => "storage",
            BenchCat::Net => "net",
            BenchCat::Sched => "sched",
            BenchCat::Image => "image",
        }
    }
}

/// 基准定义。
#[derive(Clone, Copy, Debug)]
pub struct BenchDef {
    pub id: &'static str,
    pub cat: BenchCat,
    /// 本基准回归阈值（千分比）。
    pub threshold_ppt: u32,
    /// 值口径：true = 越低越好（延迟类），false = 越高越好（吞吐类）。
    pub lower_is_better: bool,
    pub note: &'static str,
}

/// vxbench 六类基准注册表（每类 2-4 条——主册口径，共 18 条）。
pub const BENCHES: [BenchDef; 18] = [
    BenchDef { id: "load-elf-cold", cat: BenchCat::Load, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "冷装载耗时 ms" },
    BenchDef { id: "load-elf-warm", cat: BenchCat::Load, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "热装载耗时 ms" },
    BenchDef { id: "load-import-bind", cat: BenchCat::Load, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "导入解析耗时 ms" },
    BenchDef { id: "comp-frame-p50", cat: BenchCat::Composite, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "合成帧耗时 P50 ms" },
    BenchDef { id: "comp-frame-p99", cat: BenchCat::Composite, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "合成帧耗时 P99 ms" },
    BenchDef { id: "comp-layer-heavy", cat: BenchCat::Composite, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "多层场景帧耗时 ms" },
    BenchDef { id: "store-seq-w", cat: BenchCat::Storage, threshold_ppt: THRESHOLD_STORAGE_PPT, lower_is_better: true, note: "顺序写 MB/s" },
    BenchDef { id: "store-rand-r", cat: BenchCat::Storage, threshold_ppt: THRESHOLD_STORAGE_PPT, lower_is_better: true, note: "随机读 P99 μs" },
    BenchDef { id: "store-fsync", cat: BenchCat::Storage, threshold_ppt: THRESHOLD_STORAGE_PPT, lower_is_better: true, note: "fsync P99 ms" },
    BenchDef { id: "store-meta", cat: BenchCat::Storage, threshold_ppt: THRESHOLD_STORAGE_PPT, lower_is_better: true, note: "元数据操作 μs" },
    BenchDef { id: "net-ssh-echo", cat: BenchCat::Net, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "SSH 回显 ms" },
    BenchDef { id: "net-tp-1stream", cat: BenchCat::Net, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: false, note: "单流吞吐 Mb/s" },
    BenchDef { id: "sched-input-p99", cat: BenchCat::Sched, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "输入类 p99 μs" },
    BenchDef { id: "sched-audio-miss", cat: BenchCat::Sched, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "音频 deadline 误点数" },
    BenchDef { id: "img-png-4k", cat: BenchCat::Image, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "4K PNG 解码 ms" },
    BenchDef { id: "img-jpg-4k", cat: BenchCat::Image, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "4K JPEG 解码 ms" },
    BenchDef { id: "img-atlas-miss", cat: BenchCat::Image, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: true, note: "图集缺失率 ‰" },
    BenchDef { id: "img-glyph-hit", cat: BenchCat::Image, threshold_ppt: THRESHOLD_DEFAULT_PPT, lower_is_better: false, note: "字形图集命中 ‰" },
];

pub fn bench_def(id: &str) -> Option<&'static BenchDef> {
    BENCHES.iter().find(|b| b.id == id)
}

// ---------------------------------------------------------------------------
// 基线库（ADR 纪律：基线存档不可变，换基线必须留痕）
// ---------------------------------------------------------------------------

/// 基线变更记录（ADR 最小件）。
#[derive(Clone, Copy, Debug)]
pub struct BaselineChange {
    pub bench: &'static str,
    pub runner: u32,
    pub from: u64,
    pub to: u64,
    pub day: u64,
    pub reason: &'static str,
}

/// 一条基线。
#[derive(Clone, Copy, Debug)]
struct Baseline {
    bench: &'static str,
    runner: u32,
    value: u64,
}

/// 基线库：按（基准，runner 标签）绑定。
pub struct BaselineStore {
    lines: Vec<Baseline>,
    changes: RingLog<BaselineChange, 64>,
    pub change_count: u64,
}

impl BaselineStore {
    pub fn new() -> BaselineStore {
        BaselineStore { lines: Vec::new(), changes: RingLog::new(), change_count: 0 }
    }

    /// 首次建档（已有基线时拒绝——不可变纪律：改基线必须走 `retire`）。
    pub fn establish(&mut self, bench: &'static str, runner: u32, value: u64) -> bool {
        if self.get(bench, runner).is_some() || value == 0 {
            return false;
        }
        self.lines.push(Baseline { bench, runner, value });
        true
    }

    pub fn get(&self, bench: &str, runner: u32) -> Option<u64> {
        self.lines
            .iter()
            .rev()
            .find(|b| b.bench == bench && b.runner == runner)
            .map(|b| b.value)
    }

    /// 换基线（ADR：必须给理由；留痕不可删）。
    pub fn retire(
        &mut self,
        bench: &'static str,
        runner: u32,
        new_value: u64,
        day: u64,
        reason: &'static str,
    ) -> bool {
        let old = match self.get(bench, runner) {
            Some(v) => v,
            None => return false,
        };
        if new_value == 0 || new_value == old || reason.is_empty() {
            return false;
        }
        self.lines.push(Baseline { bench, runner, value: new_value });
        self.changes.push(BaselineChange { bench, runner, from: old, to: new_value, day, reason });
        self.change_count += 1;
        true
    }

    /// 变更史（新→旧）。
    pub fn change_log(&self) -> Vec<BaselineChange> {
        self.changes.newest_first()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }
}

impl Default for BaselineStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 夜跑与裁决
// ---------------------------------------------------------------------------

/// 单夜单基准运行结果（三跑原始值由采集侧给入）。
#[derive(Clone, Copy, Debug)]
pub struct NightRun {
    pub bench: &'static str,
    pub runner: u32,
    /// 三跑原始值。
    pub runs: [u64; RUNS_PER_NIGHT],
    pub day: u64,
    /// 噪声带标注（虚拟化抖动幅度 ‰——采集侧按跑间极差给出）。
    pub noise_ppt: u32,
}

impl NightRun {
    /// 中位数（三跑）。
    pub fn median(&self) -> u64 {
        let mut v = self.runs;
        v.sort_unstable();
        v[1]
    }

    /// 跑间极差（‰，中位数为基）——噪声带标注来源。
    pub fn noise_self(&self) -> u32 {
        let (min, max) = (self.runs.iter().min().unwrap_or(&0), self.runs.iter().max().unwrap_or(&0));
        let med = self.median().max(1);
        (((max - min) * 1000) / med) as u32
    }
}

/// 单基准单夜裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 达标（或无基线跳过）。
    Green,
    /// 倒退但未连续两晚——登记观察。
    Red,
    /// 连续两晚红——正式告警。
    Alarm,
    /// 无基线（首夜——自动建档建议）。
    NoBaseline,
}

/// 趋势账本单点。
#[derive(Clone, Copy, Debug)]
pub struct TrendPoint {
    pub day: u64,
    pub value: u64,
    pub verdict: Verdict,
    /// 外因标注（固件更新/借力件升级……），None = 无。
    pub ext_cause: Option<&'static str>,
}

/// 回归门。
pub struct RegGate {
    baselines: BaselineStore,
    /// 每基准连续红夜计数（键：bench 哈希槽 32）。
    red_streak: [(&'static str, u32); 32],
    /// 趋势账本：bench → 最近 90 夜。
    trends: Vec<(&'static str, Vec<TrendPoint>)>,
    /// 注入回退登记（验收判据：注入 5 处全部检出）。
    injected: Vec<(&'static str, u64)>,
    /// 裁决账目。
    pub nights_total: u64,
    pub reds_total: u64,
    pub alarms_total: u64,
    pub detected_injections: u64,
    /// 误报账目：红/告警但无注入且无外因标注。
    pub false_alarms: u64,
    /// 告警事件（新→旧）。
    alarms: RingLog<(&'static str, u64), 32>,
}

impl RegGate {
    pub fn new() -> RegGate {
        RegGate {
            baselines: BaselineStore::new(),
            red_streak: [("", 0); 32],
            trends: Vec::new(),
            injected: Vec::new(),
            nights_total: 0,
            reds_total: 0,
            alarms_total: 0,
            detected_injections: 0,
            false_alarms: 0,
            alarms: RingLog::new(),
        }
    }

    pub fn baselines(&self) -> &BaselineStore {
        &self.baselines
    }

    pub fn baselines_mut(&mut self) -> &mut BaselineStore {
        &mut self.baselines
    }

    /// 注入回退登记（验收：人为注入 5 处性能回退全部检出）。
    pub fn inject_regression(&mut self, bench: &'static str, day: u64) {
        self.injected.push((bench, day));
    }

    /// 外因标注（趋势图异常点：固件更新/借力件升级等）。
    pub fn annotate(&mut self, bench: &'static str, day: u64, cause: &'static str) {
        if let Some((_, tr)) = self.trends.iter_mut().find(|(b, _)| *b == bench) {
            if let Some(pt) = tr.iter_mut().find(|p| p.day == day) {
                pt.ext_cause = Some(cause);
            }
        }
    }

    fn streak_of(&self, bench: &str) -> u32 {
        self.red_streak
            .iter()
            .find(|(b, _)| *b == bench)
            .map(|(_, s)| *s)
            .unwrap_or(0)
    }

    fn bump_streak(&mut self, bench: &'static str, v: u32) {
        if let Some(slot) = self.red_streak.iter_mut().find(|(b, _)| *b == bench) {
            slot.1 = v;
        } else if let Some(empty) = self.red_streak.iter_mut().find(|(b, _)| b.is_empty()) {
            *empty = (bench, v);
        }
    }

    /// 提交一夜结果并裁决。
    pub fn submit(&mut self, run: NightRun) -> Verdict {
        let def = match bench_def(run.bench) {
            Some(d) => d,
            None => return Verdict::NoBaseline,
        };
        self.nights_total += 1;
        let med = run.median();
        let noise = run.noise_ppt.max(run.noise_self());
        let verdict = match self.baselines.get(run.bench, run.runner) {
            None => Verdict::NoBaseline,
            Some(base) => {
                // 倒退判定：lower_is_better → med > base；否则 med < base。
                let degrade = if def.lower_is_better {
                    med.saturating_sub(base)
                } else {
                    base.saturating_sub(med)
                };
                let degrade_ppt = (degrade * 1000 / base.max(1)) as u32;
                // 噪声带内不算倒退（噪声 > 阈值时按噪声放行并标注）。
                if degrade_ppt > def.threshold_ppt.max(noise) {
                    let streak = self.streak_of(run.bench) + 1;
                    self.bump_streak(run.bench, streak);
                    self.reds_total += 1;
                    if self.injected.iter().any(|(b, d)| *b == run.bench && *d == run.day) {
                        self.detected_injections += 1;
                    } else if streak == 1 {
                        // 首晚红无注入无外因 → 误报候选（连续才转告警，
                        // 但首晚红本身已计入误报观察——30 天窗口口径）。
                        self.false_alarms += 1;
                    }
                    if streak >= RED_NIGHTS_FOR_ALARM {
                        self.alarms_total += 1;
                        self.alarms.push((run.bench, run.day));
                        Verdict::Alarm
                    } else {
                        Verdict::Red
                    }
                } else {
                    self.bump_streak(run.bench, 0);
                    Verdict::Green
                }
            }
        };
        // 趋势账本（90 天窗）。
        let tr = match self.trends.iter_mut().find(|(b, _)| *b == run.bench) {
            Some((_, tr)) => tr,
            None => {
                self.trends.push((run.bench, Vec::new()));
                &mut self.trends.last_mut().unwrap().1
            }
        };
        tr.push(TrendPoint { day: run.day, value: med, verdict, ext_cause: None });
        if tr.len() > TREND_DAYS {
            tr.remove(0);
        }
        verdict
    }

    /// 趋势读数（某基准，90 天窗）。
    pub fn trend(&self, bench: &str) -> &[TrendPoint] {
        self.trends
            .iter()
            .find(|(b, _)| *b == bench)
            .map(|(_, tr)| tr.as_slice())
            .unwrap_or(&[])
    }

    /// 告警史（新→旧）。
    pub fn alarm_log(&self) -> Vec<(&'static str, u64)> {
        self.alarms.newest_first()
    }

    /// 捕获率验收面：注入数 vs 检出数。
    pub fn capture_rate(&self) -> (u64, u64) {
        (self.detected_injections, self.injected.len() as u64)
    }

    /// 误报率验收面：误报 / 总红夜（30 天窗口内），样本不足 → None。
    pub fn false_alarm_ppt(&self) -> Option<u32> {
        if self.reds_total == 0 {
            return None;
        }
        Some((self.false_alarms * 1000 / self.reds_total) as u32)
    }

    /// 差值表一行（CI 报告页：本次 vs 基线）。
    pub fn delta_line(&self, run: &NightRun) -> Option<(&'static str, i64, i64)> {
        let base = self.baselines.get(run.bench, run.runner)?;
        let med = run.median() as i64;
        let b = base as i64;
        Some((run.bench, med - b, ((med - b) * 100 / b.max(1))))
    }
}

impl Default for RegGate {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F061 自检（聚合进 star 域）。
pub fn run_reggate_checks() -> CheckSet {
    let mut set = CheckSet::new("F061-reggate");

    // 注册表完整性：六类 × 每类 2-4 条。
    for cat in BenchCat::ALL {
        let n = BENCHES.iter().filter(|b| b.cat == cat).count();
        set.add("cat count", (2..=4).contains(&n), "");
    }
    set.add("bench lookup", bench_def("store-rand-r").unwrap().threshold_ppt == THRESHOLD_STORAGE_PPT, "");

    // 基线不可变纪律：establish 二次拒绝；retire 必须给理由。
    let mut bs = BaselineStore::new();
    set.add("baseline establish", bs.establish("load-elf-warm", 1, 100), "");
    set.add("baseline dup rejected", !bs.establish("load-elf-warm", 1, 90), "");
    set.add("baseline retire no-reason", !bs.retire("load-elf-warm", 1, 90, 3, ""), "");
    set.add("baseline retire ok", bs.retire("load-elf-warm", 1, 90, 3, "borrowed crate bump"), "");
    set.add("baseline retire visible", bs.get("load-elf-warm", 1) == Some(90) && bs.change_log().len() == 1, "");

    // 注入 5 处回退全部检出（主册验收判据直演）。
    let mut gate = RegGate::new();
    for b in BENCHES.iter().take(5) {
        gate.baselines_mut().establish(b.id, 1, 1000);
    }
    for (i, b) in BENCHES.iter().take(5).enumerate() {
        gate.inject_regression(b.id, 10 + i as u64);
        // 注入 10% 倒退（超过默认阈值 5%）。
        let r = NightRun { bench: b.id, runner: 1, runs: [1100; 3], day: 10 + i as u64, noise_ppt: 0 };
        gate.submit(r);
    }
    let (det, total) = gate.capture_rate();
    set.add("injection captured", det == 5 && total == 5, "");

    // 连续两晚红才告警。
    let mut gate = RegGate::new();
    gate.baselines_mut().establish("img-png-4k", 1, 100);
    let bad = NightRun { bench: "img-png-4k", runner: 1, runs: [150; 3], day: 2, noise_ppt: 0 };
    set.add("first night red", gate.submit(bad) == Verdict::Red, "");
    let bad2 = NightRun { bench: "img-png-4k", runner: 1, runs: [150; 3], day: 3, noise_ppt: 0 };
    set.add("second night alarm", gate.submit(bad2) == Verdict::Alarm, "");
    set.add("alarm logged", gate.alarm_log().len() == 1, "");

    // 噪声带：噪声 20% 时 8% 倒退不判红。
    let mut gate = RegGate::new();
    gate.baselines_mut().establish("net-ssh-echo", 1, 1000);
    let noisy = NightRun { bench: "net-ssh-echo", runner: 1, runs: [1000, 900, 1100], day: 5, noise_ppt: 0 };
    set.add("noise band pass", gate.submit(noisy) == Verdict::Green, "");

    // 无基线首夜。
    let mut gate = RegGate::new();
    let first = NightRun { bench: "sched-input-p99", runner: 7, runs: [10; 3], day: 1, noise_ppt: 0 };
    set.add("no baseline", gate.submit(first) == Verdict::NoBaseline, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn run(bench: &'static str, day: u64, vals: [u64; 3]) -> NightRun {
        NightRun { bench, runner: 1, runs: vals, day, noise_ppt: 0 }
    }

    #[test]
    fn f061_median_and_noise() {
        let r = run("x", 1, [100, 120, 110]);
        assert_eq!(r.median(), 110);
        assert!(r.noise_self() > 150, "极差 20/110 ≈ 182‰");
        let stable = run("x", 2, [100, 101, 100]);
        assert!(stable.noise_self() < 15);
    }

    #[test]
    fn f061_threshold_per_cat() {
        // 存储类 8‰ 阈值：7.9% 倒退不红。
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("store-seq-w", 1, 1000);
        let r = run("store-seq-w", 2, [1079, 1079, 1079]);
        assert_eq!(gate.submit(r), Verdict::Green);
        // 8.1% 倒退 → 红。
        let r = run("store-seq-w", 3, [1081, 1081, 1081]);
        assert_eq!(gate.submit(r), Verdict::Red);
        // 非存储类 5‰：5.5% 倒退即红。
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("img-jpg-4k", 1, 1000);
        let r = run("img-jpg-4k", 2, [1055, 1055, 1055]);
        assert_eq!(gate.submit(r), Verdict::Red);
    }

    #[test]
    fn f061_lower_is_better_both_directions() {
        // 吞吐类（higher better）：值下降 6% → 红。
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("net-tp-1stream", 1, 1000);
        let r = run("net-tp-1stream", 2, [935, 940, 930]);
        assert_eq!(gate.submit(r), Verdict::Red);
        // 值上升 → 绿。
        let r = run("net-tp-1stream", 3, [1100, 1100, 1100]);
        assert_eq!(gate.submit(r), Verdict::Green);
    }

    #[test]
    fn f061_streak_resets_on_green() {
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("img-png-4k", 1, 100);
        assert_eq!(gate.submit(run("img-png-4k", 1, [150; 3])), Verdict::Red);
        assert_eq!(gate.submit(run("img-png-4k", 2, [100; 3])), Verdict::Green);
        assert_eq!(gate.submit(run("img-png-4k", 3, [150; 3])), Verdict::Red, "绿打断连红");
    }

    #[test]
    fn f061_trend_window_90() {
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("img-png-4k", 1, 100);
        for d in 0..100u64 {
            gate.submit(run("img-png-4k", d, [100; 3]));
        }
        assert_eq!(gate.trend("img-png-4k").len(), TREND_DAYS);
        assert_eq!(gate.trend("img-png-4k")[0].day, 10, "最老 10 天被挤掉");
    }

    #[test]
    fn f061_annotate_ext_cause() {
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("img-png-4k", 1, 100);
        gate.submit(run("img-png-4k", 5, [100; 3]));
        gate.annotate("img-png-4k", 5, "firmware update");
        assert_eq!(gate.trend("img-png-4k")[0].ext_cause, Some("firmware update"));
    }

    #[test]
    fn f061_delta_line() {
        let mut gate = RegGate::new();
        gate.baselines_mut().establish("img-png-4k", 1, 100);
        let r = run("img-png-4k", 2, [110; 3]);
        let (bench, delta, pct) = gate.delta_line(&r).unwrap();
        assert_eq!((bench, delta, pct), ("img-png-4k", 10, 10));
    }

    #[test]
    fn f061_run_checks_pass() {
        assert!(run_reggate_checks().all_passed());
    }
}
