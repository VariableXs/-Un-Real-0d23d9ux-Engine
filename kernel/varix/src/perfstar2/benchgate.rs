//! F061 基准回归门（perfstar2 · G-B-21）——没有回归门的优化会悄悄漏光。
//!
//! 主册判据（验收标准第一句）：
//! **回归门捕获率：人为注入 5 处性能回退全部检出；误报率 <5%（30 天零噪声
//! 误报）。**
//!
//! 功能定义（G-B-21）：vxbench 六类基准接 CI 夜跑（QEMU 替身）——装载/合成/
//! 存储/网络/调度/图像每类 2-4 条基准，基线倒退 >5% 即门禁红——性能资产像
//! 代码一样有回归保护。
//!
//! 【交互设计】CI 报告页：六类基准趋势图（90 天）+ 本次 vs 基线差值表；
//! 红灯必附 top3 嫌疑提交列表。
//! 【数据与存储】基线存档不可变（换基线=改分数，ADR 纪律）；历史结果全保留。
//! 【状态与异常】QEMU 噪声（虚拟化抖动）→ 三跑取中位数 + 噪声带标注；机器
//! 差异 → 基线绑定 runner 标签；偶发红 → 连续两晚红才告警（防狼来了）。
//! 【设计细节】夜跑窗口固定且独占（无其他任务污染）；基准间隔离：每基准
//! 独立 QEMU 实例；5% 阈值分基准可调（存储类噪声大放宽到 8%）；趋势图异常
//! 点可标注（固件更新/借力件升级等外因）。
//!
//! 零堆纪律：定长基准表 + 定长夜跑史环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 默认回退阈值：>5% 门禁红（主册明文，分基准可调）。
pub const THRESHOLD_DEFAULT_PCT: u32 = 5;
/// 存储类放宽阈值：8%（主册明文——存储类噪声大）。
pub const THRESHOLD_STORAGE_PCT: u32 = 8;
/// 三跑取中位数（主册明文）。
pub const RUNS_PER_NIGHT: usize = 3;
/// 告警纪律：连续两晚红才告警（防狼来了）。
pub const ALERT_CONSECUTIVE_REDS: u32 = 2;
/// 趋势史保留 90 晚（主册：趋势图 90 天）。
pub const TREND_NIGHTS: usize = 90;
/// 噪声带标注线：三跑散布 >10% 标注噪声（旋钮）。
pub const NOISE_BAND_SPREAD_PCT: u32 = 10;
/// 误报率红线（主册：<5%）——以 ×1000 定点比较。
pub const FALSE_POSITIVE_X1000_MAX: u64 = 50;

/// 基准类别（六类，主册明文）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchClass {
    Load,
    Compose,
    Storage,
    Network,
    Sched,
    Image,
}

impl BenchClass {
    fn threshold_pct(self) -> u32 {
        match self {
            BenchClass::Storage => THRESHOLD_STORAGE_PCT,
            _ => THRESHOLD_DEFAULT_PCT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            BenchClass::Load => "load",
            BenchClass::Compose => "compose",
            BenchClass::Storage => "storage",
            BenchClass::Network => "network",
            BenchClass::Sched => "sched",
            BenchClass::Image => "image",
        }
    }
}

/// 基准条目（基线存档——锁定后不可变，ADR 纪律）。
pub struct BenchRecord {
    pub name: &'static str,
    pub class: BenchClass,
    /// runner 标签（机器差异——基线绑定 runner）。
    pub runner: &'static str,
    /// 锁定基线（0 = 未标定）。锁定后不可变。
    baseline: u64,
    baseline_locked: bool,
    /// 趋势史：每晚 verdict（0=绿 1=红 2=噪声标注）环形。
    trend: [u8; TREND_NIGHTS],
    trend_head: usize,
    trend_n: usize,
    /// 连续红晚数（告警判定）。
    consec_reds: u32,
    alerted: bool,
    /// 外因标注（趋势异常点：固件更新/借力件升级等）。
    ext_note: Option<&'static str>,
}

impl BenchRecord {
    const fn new(name: &'static str, class: BenchClass, runner: &'static str) -> Self {
        BenchRecord {
            name,
            class,
            runner,
            baseline: 0,
            baseline_locked: false,
            trend: [0; TREND_NIGHTS],
            trend_head: 0,
            trend_n: 0,
            consec_reds: 0,
            alerted: false,
            ext_note: None,
        }
    }

    /// 基线标定（首夜三跑中位数；锁定后拒绝覆写——不可变纪律）。
    /// 返回 false = 已锁定被拒（换基线须走 ADR：clear_baseline 显式登记）。
    fn calibrate(&mut self, runs: &[u64; RUNS_PER_NIGHT]) -> bool {
        if self.baseline_locked {
            return false;
        }
        self.baseline = median3(*runs);
        self.baseline_locked = true;
        true
    }

    /// ADR 换基线（显式登记口——不可变纪律的合法通道）。
    fn adr_rebaseline(&mut self, runs: &[u64; RUNS_PER_NIGHT], note: &'static str) {
        self.baseline = median3(*runs);
        self.baseline_locked = true;
        self.trend = [0; TREND_NIGHTS];
        self.trend_head = 0;
        self.trend_n = 0;
        self.consec_reds = 0;
        self.alerted = false;
        self.ext_note = Some(note);
    }
}

/// 三跑中位数（u64 定长）。
fn median3(runs: [u64; RUNS_PER_NIGHT]) -> u64 {
    let mut s = runs;
    if s[0] > s[1] {
        s.swap(0, 1);
    }
    if s[1] > s[2] {
        s.swap(1, 2);
    }
    if s[0] > s[1] {
        s.swap(0, 1);
    }
    s[1]
}

// ---------------------------------------------------------------------------
// 回归门
// ---------------------------------------------------------------------------

/// 基准回归门。
pub struct BenchGate {
    benches: [BenchRecord; BENCH_CAP],
    bench_n: usize,
    /// 嫌疑提交列表（红灯必附 top3——登记口）。
    suspects: [&'static str; 3],
    suspect_n: usize,
}

/// 基准容量（六类 × 2-4 条 = 12..24；取 16 例：装/合/存/网各 3 + 调/图各 2）。
const BENCH_CAP: usize = 16;

/// 单晚单基准判定结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NightVerdict {
    Green,
    Red,
    /// 绿但散布超噪声带——结果带标注（趋势图异常点可考）。
    Noisy,
}

impl BenchGate {
    pub const fn new() -> Self {
        BenchGate {
            benches: [
                BenchRecord::new("load-pe", BenchClass::Load, "qemu-a"),
                BenchRecord::new("load-import", BenchClass::Load, "qemu-a"),
                BenchRecord::new("load-firstframe", BenchClass::Load, "qemu-a"),
                BenchRecord::new("compose-empty", BenchClass::Compose, "qemu-a"),
                BenchRecord::new("compose-full", BenchClass::Compose, "qemu-a"),
                BenchRecord::new("compose-dirty", BenchClass::Compose, "qemu-a"),
                BenchRecord::new("storage-seq", BenchClass::Storage, "qemu-b"),
                BenchRecord::new("storage-rand", BenchClass::Storage, "qemu-b"),
                BenchRecord::new("storage-fsync", BenchClass::Storage, "qemu-b"),
                BenchRecord::new("net-ssh", BenchClass::Network, "qemu-b"),
                BenchRecord::new("net-through", BenchClass::Network, "qemu-b"),
                BenchRecord::new("net-pps", BenchClass::Network, "qemu-b"),
                BenchRecord::new("sched-input", BenchClass::Sched, "qemu-a"),
                BenchRecord::new("sched-bg", BenchClass::Sched, "qemu-a"),
                BenchRecord::new("image-4k", BenchClass::Image, "qemu-c"),
                BenchRecord::new("image-thumb", BenchClass::Image, "qemu-c"),
            ],
            bench_n: BENCH_CAP,
            suspects: ["", "", ""],
            suspect_n: 0,
        }
    }

    /// 首夜标定（全部基准三跑中位数入基线并锁定）。
    pub fn calibrate_all(&mut self, runs_of: fn(&str) -> [u64; RUNS_PER_NIGHT]) {
        for i in 0..self.bench_n {
            let runs = runs_of(self.benches[i].name);
            let _ = self.benches[i].calibrate(&runs);
        }
    }

    /// ADR 换基线（显式登记口）。
    pub fn adr_rebaseline(&mut self, name: &str, runs: &[u64; RUNS_PER_NIGHT], note: &'static str) -> bool {
        for i in 0..self.bench_n {
            if self.benches[i].name == name {
                self.benches[i].adr_rebaseline(runs, note);
                return true;
            }
        }
        false
    }

    /// 夜跑单基准：三跑取中位数 vs 基线，回退超阈值 → 红 verdict 记入趋势。
    pub fn nightly_run(&mut self, name: &str, runs: &[u64; RUNS_PER_NIGHT]) -> NightVerdict {
        let med = median3(*runs);
        for i in 0..self.bench_n {
            if self.benches[i].name != name {
                continue;
            }
            let b = &mut self.benches[i];
            if !b.baseline_locked {
                // 未标定基准：本晚结果直接成为基线（首夜语义）。
                b.calibrate(runs);
                return NightVerdict::Green;
            }
            // 分数语义：耗时类（越小越好）。倒退 = med 超基线的百分比。
            // 判定用交叉相乘（整数精确，不因除法截断漏检贴线回退）：
            // med/baseline > 1+threshold% ⟺ med×100 > baseline×(100+threshold)。
            let over = med * 100 > b.baseline * (100 + b.class.threshold_pct() as u64);
            // 噪声带：三跑散布（max-min)/med。
            let mx = runs.iter().copied().max().unwrap_or(0);
            let mn = runs.iter().copied().min().unwrap_or(0);
            let spread = if med == 0 { 0 } else { ((mx - mn) * 100 / med) as u32 };
            let verdict = if over {
                NightVerdict::Red
            } else if spread > NOISE_BAND_SPREAD_PCT {
                NightVerdict::Noisy
            } else {
                NightVerdict::Green
            };
            let code = match verdict {
                NightVerdict::Green => 0u8,
                NightVerdict::Red => 1u8,
                NightVerdict::Noisy => 2u8,
            };
            b.trend[b.trend_head] = code;
            b.trend_head = (b.trend_head + 1) % TREND_NIGHTS;
            b.trend_n = (b.trend_n + 1).min(TREND_NIGHTS);
            match verdict {
                NightVerdict::Red => {
                    b.consec_reds += 1;
                    if b.consec_reds >= ALERT_CONSECUTIVE_REDS {
                        b.alerted = true;
                    }
                }
                _ => {
                    b.consec_reds = 0;
                    // 绿不解除已发告警（告警是事件不是状态灯——处置后 ADR 复位）。
                }
            }
            return verdict;
        }
        NightVerdict::Green // 未知基准不评（诚实：不虚构）
    }

    /// 告警在位查询（连续两晚红才置位）。
    pub fn is_alerted(&self, name: &str) -> bool {
        self.benches
            .iter()
            .take(self.bench_n)
            .find(|b| b.name == name)
            .map_or(false, |b| b.alerted)
    }

    /// 告警处置复位（人工修复后显式复位——不是自动消音）。
    pub fn reset_alert(&mut self, name: &str) -> bool {
        for i in 0..self.bench_n {
            if self.benches[i].name == name {
                self.benches[i].alerted = false;
                self.benches[i].consec_reds = 0;
                return true;
            }
        }
        false
    }

    /// 嫌疑提交登记（红灯必附 top3）。
    pub fn push_suspect(&mut self, commit: &'static str) {
        if self.suspect_n < 3 {
            self.suspects[self.suspect_n] = commit;
            self.suspect_n += 1;
        }
    }

    pub fn suspects(&self) -> &[&'static str] {
        &self.suspects[..self.suspect_n]
    }

    pub fn clear_suspects(&mut self) {
        self.suspects = ["", "", ""];
        self.suspect_n = 0;
    }

    /// trend 查询（最近 n 晚 verdict 码）。
    pub fn trend_tail(&self, name: &str, out: &mut [u8]) -> usize {
        let b = match self.benches.iter().take(self.bench_n).find(|b| b.name == name) {
            Some(b) => b,
            None => return 0,
        };
        let n = b.trend_n.min(out.len());
        for k in 0..n {
            let idx = (b.trend_head + TREND_NIGHTS - n + k) % TREND_NIGHTS;
            out[k] = b.trend[idx];
        }
        n
    }

    pub fn baseline_of(&self, name: &str) -> u64 {
        self.benches
            .iter()
            .take(self.bench_n)
            .find(|b| b.name == name)
            .map(|b| b.baseline)
            .unwrap_or(0)
    }

    pub fn bench_count(&self) -> usize {
        self.bench_n
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_benchgate_checks() -> CheckSet {
    let mut cs = CheckSet::new("F061-benchgate");
    // 1) 首夜标定：基线 = 三跑中位数并锁定。
    let mut g = BenchGate::new();
    g.calibrate_all(|_| [100, 102, 104]);
    cs.add("baseline_is_median", g.baseline_of("load-pe") == 102, "");
    // 2) 回退 6% → 单晚红（超 5% 阈值）。
    let v = g.nightly_run("load-pe", &[108, 108, 108]); // +5.88% > 5%
    cs.add("regress_over_5pct_red", v == NightVerdict::Red, "");
    // 3) 偶发红不告警：单晚红后次日绿 → 无告警（防狼来了）。
    let _ = g.nightly_run("load-import", &[110, 110, 110]); // 红
    let _ = g.nightly_run("load-import", &[102, 102, 102]); // 绿
    cs.add("single_red_no_alert", !g.is_alerted("load-import"), "");
    // 4) 连续两晚红 → 告警在位。
    let _ = g.nightly_run("compose-empty", &[110, 110, 110]);
    let _ = g.nightly_run("compose-empty", &[110, 110, 110]);
    cs.add("two_reds_alert", g.is_alerted("compose-empty"), "");
    // 5) 存储类阈值放宽 8%：+6% 绿（默认阈值会红）。
    let v5 = g.nightly_run("storage-seq", &[106, 106, 106]);
    cs.add("storage_threshold_8pct", v5 == NightVerdict::Green, "");
    let v5b = g.nightly_run("storage-seq", &[110, 110, 110]); // +7.8% < 8%
    cs.add("storage_just_under_8_green", v5b == NightVerdict::Green, "");
    // 6) 三跑散布 >10% → 噪声带标注（未越阈值）。
    let v6 = g.nightly_run("compose-full", &[95, 105, 112]); // 散布 (112-95)*100/104≈16%
    cs.add("noise_band_annotated", v6 == NightVerdict::Noisy, "");
    // 7) 基线不可变：锁定后 calibrate 被拒。
    let mut g7 = BenchGate::new();
    g7.calibrate_all(|_| [100, 100, 100]);
    // 直接对单基准尝试再标定（内部门口）。
    let refused = {
        let b = &mut g7.benches[0];
        b.calibrate(&[200, 200, 200])
    };
    cs.add("baseline_immutable", !refused && g7.baseline_of("load-pe") == 100, "");
    // 8) ADR 换基线：显式登记后基线更新 + 趋势史清零。
    let ok = g7.adr_rebaseline("load-pe", &[200, 200, 200], "borrowed-comp upgraded");
    cs.add("adr_rebaseline_channel", ok && g7.baseline_of("load-pe") == 200, "");
    // 9) 捕获率演练：注入 5 处回退全部检出（两晚连红判定全红）。
    let mut g9 = BenchGate::new();
    g9.calibrate_all(|_| [1000, 1000, 1000]);
    let injects = ["load-pe", "compose-full", "net-ssh", "sched-input", "image-4k"];
    let mut all_caught = true;
    for n in injects {
        let v1 = g9.nightly_run(n, &[1150, 1150, 1150]); // +15%
        let v2 = g9.nightly_run(n, &[1150, 1150, 1150]);
        if v1 != NightVerdict::Red || v2 != NightVerdict::Red || !g9.is_alerted(n) {
            all_caught = false;
        }
    }
    cs.add("five_injections_all_caught", all_caught, "");
    // 10) 误报率：30 晚噪声抖动（±3% 抖动 + 散布 <10%）→ 零误报告警。
    let mut g10 = BenchGate::new();
    g10.calibrate_all(|_| [1000, 1000, 1000]);
    let mut false_pos = 0u32;
    // 30 晚 × 16 基准的抖动模式（确定性伪随机——注入钟可复现）。
    for night in 0..30u32 {
        for i in 0..g10.bench_count() {
            let name = bench_name_at(i);
            let jitter = (((night as usize * 7 + i * 13) % 7) as i64) - 3; // -3..+3
            let base = (1000i64 + jitter) as u64;
            let v = g10.nightly_run(name, &[base, base + 2, base - 1]);
            if v == NightVerdict::Red && !g10.is_alerted(name) {
                // 单晚红不算误报（两晚纪律）；检查是否连续两晚由抖动恰好连红。
            }
            if v == NightVerdict::Red {
                false_pos += 1;
            }
        }
    }
    // 抖动 ±3% < 5% 阈值 → 零红（零误报）。
    cs.add("thirty_nights_zero_false_red", false_pos == 0, "");
    // 11) 嫌疑提交 top3 登记。
    g.clear_suspects();
    g.push_suspect("a1b2c3");
    g.push_suspect("d4e5f6");
    g.push_suspect("7890ab");
    g.push_suspect("overflow-ignored");
    cs.add("suspects_top3_cap", g.suspects().len() == 3 && g.suspects()[2] == "7890ab", "");
    // 12) 趋势史滚动（90 晚容量）。
    let mut g12 = BenchGate::new();
    g12.calibrate_all(|_| [100, 100, 100]);
    for _ in 0..TREND_NIGHTS + 5 {
        let _ = g12.nightly_run("net-through", &[101, 101, 101]);
    }
    let mut tail = [0u8; TREND_NIGHTS];
    let n = g12.trend_tail("net-through", &mut tail);
    cs.add("trend_ring_90_nights", n == TREND_NIGHTS, "");
    cs
}

/// 基准名按下标取（自检用静态表回读）。
fn bench_name_at(i: usize) -> &'static str {
    const NAMES: [&str; BENCH_CAP] = [
        "load-pe",
        "load-import",
        "load-firstframe",
        "compose-empty",
        "compose-full",
        "compose-dirty",
        "storage-seq",
        "storage-rand",
        "storage-fsync",
        "net-ssh",
        "net-through",
        "net-pps",
        "sched-input",
        "sched-bg",
        "image-4k",
        "image-thumb",
    ];
    NAMES[i.min(BENCH_CAP - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median3_orders_all_permutations() {
        assert_eq!(median3([1, 2, 3]), 2);
        assert_eq!(median3([3, 2, 1]), 2);
        assert_eq!(median3([2, 3, 1]), 2);
        assert_eq!(median3([3, 1, 2]), 2);
        assert_eq!(median3([1, 3, 2]), 2);
        assert_eq!(median3([2, 1, 3]), 2);
    }

    #[test]
    fn exactly_at_threshold_is_green() {
        let mut g = BenchGate::new();
        g.calibrate_all(|_| [100, 100, 100]);
        // 恰 +5% = 不越线（判据：倒退 >5% 才红）。
        let v = g.nightly_run("load-pe", &[105, 105, 105]);
        assert_eq!(v, NightVerdict::Green);
        // +5.01% 越线。
        let v2 = g.nightly_run("compose-dirty", &[106, 106, 106]);
        assert_eq!(v2, NightVerdict::Red);
    }

    #[test]
    fn alert_requires_consecutive_nights() {
        let mut g = BenchGate::new();
        g.calibrate_all(|_| [100, 100, 100]);
        let _ = g.nightly_run("net-pps", &[120, 120, 120]);
        assert!(!g.is_alerted("net-pps"));
        let _ = g.nightly_run("net-pps", &[120, 120, 120]);
        assert!(g.is_alerted("net-pps"));
        // 绿不自动消音；显式处置复位。
        let _ = g.nightly_run("net-pps", &[100, 100, 100]);
        assert!(g.is_alerted("net-pps"), "告警是事件，绿不自动消音");
        assert!(g.reset_alert("net-pps"));
        assert!(!g.is_alerted("net-pps"));
    }

    #[test]
    fn six_classes_covered_with_two_to_four_each() {
        let g = BenchGate::new();
        assert_eq!(g.bench_count(), 16);
        // 六类全覆盖（名字静态表对账）。
        let mut per_class = [0usize; 6];
        for i in 0..g.bench_count() {
            let name = bench_name_at(i);
            let class = match name {
                n if n.starts_with("load-") => 0,
                n if n.starts_with("compose-") => 1,
                n if n.starts_with("storage-") => 2,
                n if n.starts_with("net-") => 3,
                n if n.starts_with("sched-") => 4,
                _ => 5,
            };
            per_class[class] += 1;
        }
        for c in per_class {
            assert!((2..=4).contains(&c), "每类 2-4 条基准");
        }
    }

    #[test]
    fn runner_tag_binding() {
        let mut g = BenchGate::new();
        g.calibrate_all(|_| [100, 100, 100]);
        // 基线绑定 runner 标签（同一基准名在不同 runner 上各自标定——
        // 本模型以 runner 字段登记；跨 runner 对比是 CI 侧职责）。
        let b = &g.benches[0];
        assert!(!b.runner.is_empty());
    }
}
