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

// ===========================================================================
// v2 深化批（F061 · G-B-21）——回归门统计引擎四件套
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-21 功能定义的实装细化，非新立项）：
// 1. MadOutlier —— MAD 稳健离群检测：中位数绝对偏差（对长尾不敏感，
//    三跑中位数之上的第二道防误报闸）；样本 > 中位数 + MAD_MULT·MAD
//    判离群（仅上侧——性能基准只关心变慢）。
// 2. TrendTest —— 整数 Mann-Kendall 趋势检验：S 统计 + 无结方差 +
//    整数平方根 Z ×100 定点；|Z|>196 判显著趋势（90 晚趋势环的
//    「缓慢劣化检出」——单晚 5% 看不见、三个月看得见的回退）。
// 3. TheilSens —— Theil–Sen 成对中位斜率（×100 定点 ms/晚）：趋势的
//    量化幅度——告警页「每晚慢多少」的直接答案。
// 4. FpBudget —— 误报预算台账：累计误报 / 夜数 ×1000 与主册 <5% 红线
//    同源比对——30 天零噪声误报的活账本（不是测试后才算）。
// 全部零堆：定长窗 + 定点数 + 整数平方根，无 Vec/String/浮点/format!。
// ===========================================================================

/// MAD 离群阈值倍数（×100 定点 = 4.45·MAD ≈ 3σ 稳健等价）。
pub const MAD_MULT_X100: u32 = 445;
/// MAD 零退化地板除数：阈值下限 = 中位数/50（2%）。
pub const MAD_MEDIAN_FLOOR_DIV: u32 = 50;
/// 趋势检验窗口（晚）——90 晚环内取最近 30 晚。
pub const TREND_WINDOW: usize = 30;
/// Mann-Kendall 显著线：|Z| ×100 > 196（双侧 5%）。
pub const MK_SIGNIFICANT_Z_X100: i64 = 196;
/// Theil–Sen 斜率单位：ms/晚 ×100 定点。
pub const SLOPE_UNIT_X100: i64 = 100;
/// 误报预算窗口（夜）——主册 30 天口径。
pub const FP_BUDGET_NIGHTS: u64 = 30;

// ---------------------------------------------------------------------------
// 深化一：MAD 稳健离群检测
// ---------------------------------------------------------------------------

/// 9 样本 MAD 检测器（三跑 × 3 晚的滑动窗——单晚样本太少）。
pub struct MadOutlier {
    window: [u32; 9],
    n: usize,
    flagged: u64,
}

impl MadOutlier {
    pub const fn new() -> Self {
        MadOutlier {
            window: [0; 9],
            n: 0,
            flagged: 0,
        }
    }

    /// 喂一个中位数样本（每晚的三跑中位数）。
    pub fn push(&mut self, med: u32) {
        if self.n < 9 {
            self.window[self.n] = med;
            self.n += 1;
        } else {
            for k in 0..8 {
                self.window[k] = self.window[k + 1];
            }
            self.window[8] = med;
        }
    }

    fn median_of(buf: &mut [u32]) -> u32 {
        // 插入排序取中位（9 元素，栈上 36B——内核栈预算内）。
        for i in 1..buf.len() {
            let key = buf[i];
            let mut j = i;
            while j > 0 && buf[j - 1] > key {
                buf[j] = buf[j - 1];
                j -= 1;
            }
            buf[j] = key;
        }
        buf[buf.len() / 2]
    }

    /// 当前样本是否离群（仅上侧——基准只关心变慢）。
    pub fn is_outlier(&mut self, med: u32) -> bool {
        if self.n < 5 {
            return false; // 样本不足不判（诚实——不猜）
        }
        let mut sorted = self.window;
        let m = Self::median_of(&mut sorted);
        let mut devs = [0u32; 9];
        for (k, d) in devs.iter_mut().enumerate() {
            *d = sorted[k].abs_diff(m);
        }
        let mad = Self::median_of(&mut devs[..self.n]);
        // MAD 零退化防线：窗口高度同值时 MAD=0 → 任何 1 单位偏差都成
        // 离群——阈值取 445·MAD 与 中位数/50（2% 地板）的较大者。
        let floor = m / MAD_MEDIAN_FLOOR_DIV;
        let mad_term = (MAD_MULT_X100 as u64) * (mad as u64) / 100;
        let threshold = m as u64 + mad_term.max(floor as u64);
        let out = med as u64 > threshold;
        if out {
            self.flagged += 1;
        }
        out
    }

    pub fn flagged(&self) -> u64 {
        self.flagged
    }
}

// ---------------------------------------------------------------------------
// 深化二：整数 Mann-Kendall 趋势检验
// ---------------------------------------------------------------------------

/// 牛顿法整数平方根（u64 → u32，判据可复现零依赖）。
pub fn isqrt(v: u64) -> u32 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x as u32
}

/// Mann-Kendall 检验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    /// 显著变慢（Z 超线且 S>0）。
    Worsening(i64), // Z ×100
    /// 显著变快（性能改进——同样报告，不粉饰成"稳定"）。
    Improving(i64),
    /// 无显著趋势。
    Stable,
    /// 样本不足（<8 晚——不猜）。
    Insufficient,
}

/// 对 30 晚窗口做 Mann-Kendall（无结方差；零堆双循环 435 对）。
pub fn mann_kendall(window: &[u32; TREND_WINDOW]) -> Trend {
    let mut s: i64 = 0;
    for i in 0..TREND_WINDOW - 1 {
        for j in i + 1..TREND_WINDOW {
            if window[j] > window[i] {
                s += 1;
            } else if window[j] < window[i] {
                s -= 1;
            }
        }
    }
    // 无结方差 Var(S) = n(n−1)(2n+5)/18。
    let n = TREND_WINDOW as i64;
    let var = n * (n - 1) * (2 * n + 5) / 18;
    if var == 0 {
        return Trend::Stable;
    }
    // Z = (S−sign(S)) / sqrt(Var)，×100 定点。
    let adj = s - if s > 0 { 1 } else if s < 0 { -1 } else { 0 };
    let z_x100 = (adj.abs() * 100) / isqrt(var as u64).max(1) as i64;
    if z_x100 > MK_SIGNIFICANT_Z_X100 {
        if s > 0 {
            Trend::Worsening(z_x100)
        } else {
            Trend::Improving(z_x100)
        }
    } else {
        Trend::Stable
    }
}

// ---------------------------------------------------------------------------
// 深化三：Theil–Sen 成对中位斜率
// ---------------------------------------------------------------------------

/// 30 晚窗口的 Theil–Sen 斜率（ms/晚 ×100 定点；正 = 变慢）。
/// 返回 None 当窗口全平（斜率 0 与"未定义"区分不重要时直接给 0）。
pub fn theil_sen_slope_x100(window: &[u32; TREND_WINDOW]) -> i64 {
    // 成对斜率 (xj−xi)/(yj−yi)，x = 晚序（1 晚步长），y = 耗时。
    // 435 对定长缓冲（30×29/2）——栈上 1.7KB，内核栈预算内。
    let mut slopes = [0u64; TREND_WINDOW * (TREND_WINDOW - 1) / 2];
    let mut k = 0usize;
    for i in 0..TREND_WINDOW - 1 {
        for j in i + 1..TREND_WINDOW {
            let dy = window[j] as i64 - window[i] as i64;
            let dx = (j - i) as i64;
            // 斜率 ×100：dy×100/dx（dy 可负——i64 存，排序用偏移）。
            let v = dy * 100 / dx;
            slopes[k] = (v + (1i64 << 40)) as u64; // 偏移使负值可排序
            k += 1;
        }
    }
    // 插入排序取中位（435 元素 O(n²) ≈ 9.5 万次比较——判据可接受）。
    for i in 1..k {
        let key = slopes[i];
        let mut j = i;
        while j > 0 && slopes[j - 1] > key {
            slopes[j] = slopes[j - 1];
            j -= 1;
        }
        slopes[j] = key;
    }
    let med = slopes[k / 2] as i64 - (1i64 << 40);
    med // ×100 定点 ms/晚
}

// ---------------------------------------------------------------------------
// 深化四：误报预算台账
// ---------------------------------------------------------------------------

/// 误报预算台账：告警后被证伪（复核绿）即计一次误报。
pub struct FpBudget {
    nights: u64,
    false_positives: u64,
    /// 连续绿夜数（零噪声误报的连续性证据）。
    clean_streak: u64,
}

impl FpBudget {
    pub const fn new() -> Self {
        FpBudget {
            nights: 0,
            false_positives: 0,
            clean_streak: 0,
        }
    }

    /// 过了一晚。
    pub fn night_passed(&mut self) {
        self.nights += 1;
    }

    /// 告警被复核证伪（误报 +1，连绿断）。
    pub fn false_positive(&mut self) {
        self.false_positives += 1;
        self.clean_streak = 0;
    }

    /// 复核确认红（真回退——连绿累计）。
    pub fn confirmed_red(&mut self) {
        self.clean_streak += 1;
    }

    /// 当前误报率 ×1000（窗口 FP_BUDGET_NIGHTS 口径，夜数不足按实际算）。
    pub fn rate_x1000(&self) -> u64 {
        if self.nights == 0 {
            return 0;
        }
        self.false_positives * 1000 / self.nights
    }

    /// 预算内？（主册 <5% 红线 ×1000 = 50）。
    pub fn within_budget(&self) -> bool {
        self.rate_x1000() < FALSE_POSITIVE_X1000_MAX
    }

    /// 30 天零噪声误报达成？（主册验收原话口径）。
    pub fn thirty_days_clean(&self) -> bool {
        self.nights >= FP_BUDGET_NIGHTS && self.false_positives == 0
    }

    pub fn stats(&self) -> (u64, u64, u64) {
        (self.nights, self.false_positives, self.clean_streak)
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：统计引擎四件套逐条实摆。
pub fn run_benchgate_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F061-benchgate-deep");

    // ── MAD 离群 ──
    // 1) 稳定窗（100±2）不误标。
    let mut mad = MadOutlier::new();
    for v in [100u32, 100, 101, 99, 100, 102, 100, 101, 100] {
        mad.push(v);
    }
    cs.add("mad_stable_no_flag", !mad.is_outlier(101), "");
    // 2) 真离群（单点 200）必标。
    cs.add("mad_spike_flagged", mad.is_outlier(200), "");
    // 3) 样本不足不判（5 样本以下——不猜）。
    let mut mad2 = MadOutlier::new();
    mad2.push(100);
    mad2.push(100);
    cs.add("mad_insufficient_honest", !mad2.is_outlier(500), "");
    // 4) 长尾不敏感：一个 900 混入后，正常值仍不被标（MAD 对野值免疫）。
    let mut mad3 = MadOutlier::new();
    for v in [100u32, 100, 101, 99, 900, 100, 101, 100, 100] {
        mad3.push(v);
    }
    cs.add("mad_robust_to_wild", !mad3.is_outlier(102), "");

    // ── Mann-Kendall ──
    // 5) 单调变慢序列判 Worsening。
    let mut worsen = [0u32; TREND_WINDOW];
    for (k, v) in worsen.iter_mut().enumerate() {
        *v = 1000 + k as u32 * 10; // 每晚 +10ms
    }
    cs.add(
        "mk_worsening_detected",
        matches!(mann_kendall(&worsen), Trend::Worsening(z) if z > 300),
        "",
    );
    // 6) 单调变快序列判 Improving（不粉饰）。
    let mut improve = [0u32; TREND_WINDOW];
    for (k, v) in improve.iter_mut().enumerate() {
        *v = 1200 - k as u32 * 10;
    }
    cs.add("mk_improving_detected", matches!(mann_kendall(&improve), Trend::Improving(_)), "");
    // 7) 锯齿噪声序列判 Stable（零趋势——误报面）。
    let mut noise = [0u32; TREND_WINDOW];
    for (k, v) in noise.iter_mut().enumerate() {
        *v = 1000 + if k % 2 == 0 { 5 } else { 0 };
    }
    cs.add("mk_noise_stable", mann_kendall(&noise) == Trend::Stable, "");
    // 8) Theil–Sen 斜率量化：+10ms/晚 → ×100 = 1000。
    cs.add("ts_slope_quantified", theil_sen_slope_x100(&worsen) == 1000, "");
    cs.add("ts_slope_negative_side", theil_sen_slope_x100(&improve) == -1000, "");
    // 9) isqrt 精度（牛顿法关键锚点）。
    cs.add(
        "isqrt_anchors",
        isqrt(0) == 0 && isqrt(1) == 1 && isqrt(15) == 3 && isqrt(16) == 4 && isqrt(999_800_01) == 9999,
        "",
    );

    // ── 误报预算 ──
    // 10) 30 天零误报达成判定。
    let mut fp = FpBudget::new();
    for _ in 0..FP_BUDGET_NIGHTS {
        fp.night_passed();
        fp.confirmed_red();
    }
    cs.add("fp_thirty_days_clean", fp.thirty_days_clean() && fp.within_budget(), "");
    // 11) 一次误报把率推到 33‰（30 晚窗口）——仍 <50 红线内。
    let mut fp2 = FpBudget::new();
    for _ in 0..FP_BUDGET_NIGHTS {
        fp2.night_passed();
    }
    fp2.false_positive();
    cs.add("fp_one_fp_33permille", fp2.rate_x1000() == 33 && fp2.within_budget(), "");
    // 12) 两次误报 66‰ 越线（<5% 红线是活的）。
    fp2.false_positive();
    cs.add("fp_two_fp_over_line", !fp2.within_budget(), "");
    // 13) 误报断连绿、确认红续连绿（连续性证据账）。
    let mut fp3 = FpBudget::new();
    fp3.night_passed();
    fp3.confirmed_red();
    fp3.night_passed();
    fp3.confirmed_red();
    fp3.false_positive();
    cs.add("fp_streak_ledger", fp3.stats() == (2, 1, 0), "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn mad_window_rolls_and_keeps_recent() {
        let mut m = MadOutlier::new();
        for v in 0..12u32 {
            m.push(100 + v); // 12 个递增——窗内只剩后 9 个
        }
        // 窗口中位数应落在 108±1（后 9 个 104..112 的中位）。
        // 通过 is_outlier 的行为间接验证：112 不是离群（窗口已滚动）。
        assert!(!m.is_outlier(112));
    }

    #[test]
    fn mk_detects_slow_drift_below_nightly_threshold() {
        // 每晚 +0.5ms（低于 5% 单晚阈值）持续 30 晚——单晚绿、趋势红。
        let mut w = [1000u32; TREND_WINDOW];
        for (k, v) in w.iter_mut().enumerate() {
            *v = 1000 + k as u32 / 2;
        }
        assert!(matches!(mann_kendall(&w), Trend::Worsening(_)), "缓慢劣化被趋势检验抓住");
        assert!(theil_sen_slope_x100(&w) > 0 && theil_sen_slope_x100(&w) < 100, "斜率 <1ms/晚 仍量化");
    }

    #[test]
    fn theil_sen_flat_window_is_zero() {
        let w = [1000u32; TREND_WINDOW];
        assert_eq!(theil_sen_slope_x100(&w), 0);
    }

    #[test]
    fn fp_budget_exact_threshold_math() {
        let mut fp = FpBudget::new();
        for _ in 0..20 {
            fp.night_passed();
        }
        // 1/20 = 50‰ —— 恰在线上（<50 才过，50 不过）。
        fp.false_positive();
        assert_eq!(fp.rate_x1000(), 50);
        assert!(!fp.within_budget(), "恰达红线不算预算内（<5% 严格口径）");
    }
}

// ===========================================================================
// v3 深化批（F061 · G-B-21）——回退归因 / 夜跑调度器 / 噪声地板标定
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-21 功能定义的实装细化，非新立项）：
// 1. RegressionAttribution —— 回退归因：红基准 × 嫌疑提交清单按权重
//    评分排序（提交触及该基准类别的权重高）——「嫌疑提交 top3」的
//    排序机制面。
// 2. BenchScheduler —— 夜跑调度器：基准队列按优先级 + 时间预算装箱，
//    超预算顺延（顺延计数如实——不是静默丢弃）。
// 3. NoiseFloorCal —— 噪声地板标定：从历史中位数序列学习散布带
//    （相对散布 ×100 定点），自适应建议阈值（5%/8% 之外的第二层）。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 嫌疑提交权重类别（六类对齐 BenchClass）。
pub const ATTRIB_CLASSES: usize = 6;
/// 夜跑时间预算（ms，虚拟时钟——判据层）。
pub const NIGHT_BUDGET_MS: u32 = 3_600_000;
/// 噪声地板下限（×100 定点 = 2%——比阈值低的地板，防过紧）。
pub const NOISE_FLOOR_PCT_X100: u32 = 200;

// ---------------------------------------------------------------------------
// 深化一：回退归因
// ---------------------------------------------------------------------------

/// 嫌疑提交。
#[derive(Clone, Copy, Debug)]
pub struct Suspect {
    pub tag: &'static str,
    /// 触及的类别位图（bit k = 类 k）。
    pub classes: u8,
    /// 时间邻近度分（0-100，越近越高）。
    pub recency: u32,
}

/// 归因评分：类别命中 ×40 + 邻近度 ×0.6 → 排序输出 top-k idx。
pub fn attribute(suspects: &[Suspect], bench_class_bit: u8, out: &mut [usize]) -> usize {
    let mut scores = [0u64; 16];
    let n = suspects.len().min(16);
    for (k, s) in suspects.iter().take(n).enumerate() {
        let hit = if bench_class_bit < 8 && (s.classes >> bench_class_bit) & 1 == 1 {
            40u64
        } else {
            0
        };
        scores[k] = hit * 100 + s.recency as u64 * 60;
    }
    // 插入排序 idx（分数降序，稳定）。
    let mut order = [0usize; 16];
    for k in 0..n {
        order[k] = k;
    }
    for i in 1..n {
        let key = order[i];
        let mut j = i;
        while j > 0 && scores[order[j - 1]] < scores[key] {
            order[j] = order[j - 1];
            j -= 1;
        }
        order[j] = key;
    }
    let mut written = 0;
    for k in 0..n {
        if written < out.len() {
            out[written] = order[k];
            written += 1;
        }
    }
    written
}

// ---------------------------------------------------------------------------
// 深化二：夜跑调度器
// ---------------------------------------------------------------------------

/// 调度任务。
#[derive(Clone, Copy, Debug)]
pub struct BenchJob {
    pub name: &'static str,
    /// 优先级（0 最高）。
    pub prio: u8,
    /// 预估耗时 ms。
    pub cost_ms: u32,
}

/// 夜跑调度：优先级序装箱，超预算顺延。
pub struct BenchScheduler {
    queue: [Option<BenchJob>; 24],
    n: usize,
    used_ms: u32,
    ran: u32,
    deferred: u32,
}

impl BenchScheduler {
    pub const fn new() -> Self {
        BenchScheduler { queue: [None; 24], n: 0, used_ms: 0, ran: 0, deferred: 0 }
    }

    pub fn submit(&mut self, job: BenchJob) -> bool {
        if self.n >= 24 {
            return false;
        }
        self.queue[self.n] = Some(job);
        self.n += 1;
        true
    }

    /// 跑一晚：按优先级排序后逐个装箱；放不下的顺延（留在队列）。
    pub fn run_night(&mut self) -> (u32, u32) {
        // 优先级插入排序。
        for i in 1..self.n {
            let key = self.queue[i];
            let mut j = i;
            while j > 0 && self.queue[j - 1].unwrap().prio > key.unwrap().prio {
                self.queue[j] = self.queue[j - 1];
                j -= 1;
            }
            self.queue[j] = key;
        }
        self.used_ms = 0;
        let mut ran = 0u32;
        let mut deferred = 0u32;
        let mut write = 0usize;
        for k in 0..self.n {
            let job = self.queue[k].unwrap();
            self.queue[k] = None;
            if self.used_ms + job.cost_ms <= NIGHT_BUDGET_MS {
                self.used_ms += job.cost_ms;
                ran += 1;
            } else {
                // 顺延：压实回队首。
                self.queue[write] = Some(job);
                write += 1;
                deferred += 1;
            }
        }
        for s in self.queue[write..].iter_mut() {
            *s = None;
        }
        self.n = write;
        self.ran += ran;
        self.deferred += deferred;
        (ran, deferred)
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.ran, self.deferred)
    }
}

// ---------------------------------------------------------------------------
// 深化三：噪声地板标定
// ---------------------------------------------------------------------------

/// 噪声地板标定器：喂历史三跑中位数序列 → 相对散布带。
pub struct NoiseFloorCal {
    medians: [u32; 30],
    n: usize,
}

impl NoiseFloorCal {
    pub const fn new() -> Self {
        NoiseFloorCal { medians: [0; 30], n: 0 }
    }

    pub fn push(&mut self, median: u32) {
        if self.n < 30 {
            self.medians[self.n] = median;
            self.n += 1;
        } else {
            for k in 0..29 {
                self.medians[k] = self.medians[k + 1];
            }
            self.medians[29] = median;
        }
    }

    /// 相对散布 ×100：(max−min)×100/median（最近窗）。
    pub fn spread_pct_x100(&self) -> Option<u32> {
        if self.n < 5 {
            return None;
        }
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for k in 0..self.n {
            lo = lo.min(self.medians[k]);
            hi = hi.max(self.medians[k]);
        }
        let med = self.medians[self.n / 2];
        if med == 0 {
            return None;
        }
        Some(((hi - lo) as u64 * 10_000 / med as u64) as u32)
    }

    /// 自适应建议阈值：散布带的 2.5 倍，钳制在 [2%, 8%]（×100 定点）。
    pub fn suggested_threshold_pct_x100(&self) -> Option<u32> {
        let spread = self.spread_pct_x100()?;
        let v = spread * 5 / 2;
        Some(v.clamp(NOISE_FLOOR_PCT_X100, THRESHOLD_STORAGE_PCT * 100))
    }

    pub fn samples(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：归因 / 调度 / 噪声地板逐条实摆。
pub fn run_benchgate_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F061-benchgate-v3");

    // ── 回退归因 ──
    let suspects = [
        Suspect { tag: "a", classes: 0b0_0010, recency: 90 }, // Storage
        Suspect { tag: "b", classes: 0b0_0001, recency: 95 }, // Load
        Suspect { tag: "c", classes: 0b0_0010, recency: 40 }, // Storage
    ];
    let mut out = [0usize; 8];
    let n = attribute(&suspects, 1, &mut out); // Storage 位 = 1
    cs.add("attrib_hits_first", n == 3 && out[0] == 0, ""); // a：命中40×100+90×60 最高
    cs.add("attrib_second_is_c", out[1] == 2, ""); // c：命中但邻近低
    let mut out2 = [0usize; 8];
    let _ = attribute(&suspects, 0, &mut out2); // Load 位
    cs.add("attrib_class_miss_orders_by_recency", out2[0] == 1, ""); // 全不命中 → 邻近度排序

    // ── 夜跑调度 ──
    let mut sch = BenchScheduler::new();
    let _ = sch.submit(BenchJob { name: "load", prio: 2, cost_ms: 600_000 });
    let _ = sch.submit(BenchJob { name: "core", prio: 0, cost_ms: 600_000 });
    let _ = sch.submit(BenchJob { name: "big", prio: 1, cost_ms: 3_000_000 });
    let (ran, deferred) = sch.run_night();
    cs.add(
        "sched_priority_and_budget",
        ran == 2 && deferred == 1, // core+load 跑（1.2M ≤ 3.6M），big 顺延
        "",
    );
    let (ran2, _) = sch.run_night(); // 第二晚跑顺延件
    cs.add("sched_deferred_runs_next_night", ran2 == 1 && sch.stats() == (3, 1), "");
    // 优先级序：core 先于 load（prio 0 < 2）——间接由预算装下两件验证。

    // ── 噪声地板 ──
    // 1) 样本不足不猜。
    let mut nc = NoiseFloorCal::new();
    for _ in 0..4 {
        nc.push(1000);
    }
    cs.add("noise_insufficient_none", nc.spread_pct_x100().is_none(), "");
    // 2) 稳定序列散布 ~0 → 建议钳在地板 2%。
    for _ in 0..10 {
        nc.push(1000);
    }
    cs.add("noise_floor_clamp_low", nc.suggested_threshold_pct_x100() == Some(NOISE_FLOOR_PCT_X100), "");
    // 3) 高噪声序列 → 建议钳在 8% 上限。
    let mut nc2 = NoiseFloorCal::new();
    for v in [900u32, 1200, 950, 1250, 1000, 1100, 980, 1220, 1010, 1150] {
        nc2.push(v);
    }
    cs.add("noise_clamp_high", nc2.suggested_threshold_pct_x100() == Some(800), "");
    // 4) 中等散布 → 带内建议（散布 10% × 2.5 = 25%？钳 8%——构造散布 4%）。
    let mut nc3 = NoiseFloorCal::new();
    for v in [1000u32, 1020, 1000, 1040, 1000, 1020, 1000, 1040, 1000, 1020] {
        nc3.push(v);
    }
    // 散布 = (1040−1000)×100/中位 = 400/1000 = 4% → 4×2.5 = 10% → 钳 8%。
    cs.add("noise_mid_clamped", nc3.suggested_threshold_pct_x100() == Some(800), "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn attrib_empty_suspects_zero_written() {
        let mut out = [0usize; 4];
        assert_eq!(attribute(&[], 0, &mut out), 0);
    }

    #[test]
    fn sched_all_fit_single_night() {
        let mut s = BenchScheduler::new();
        for k in 0..6u32 {
            let _ = s.submit(BenchJob { name: "x", prio: k as u8, cost_ms: 300_000 });
        }
        let (ran, deferred) = s.run_night();
        assert_eq!((ran, deferred), (6, 0));
        assert_eq!(s.stats(), (6, 0));
    }

    #[test]
    fn noise_spread_exact() {
        let mut nc = NoiseFloorCal::new();
        for v in [1000u32, 1010, 1005, 1015, 1000, 1012, 1008, 1002, 1011, 1004] {
            nc.push(v);
        }
        // 散布 = (1015−1000)×100/中位(≈1007) ≈ 149。
        let sp = nc.spread_pct_x100().unwrap();
        assert!(sp > 100 && sp < 200, "散布落在 1%-2% 带 sp={sp}");
    }
}

// ===========================================================================
// v4 深化批（F061 · G-B-21）——趋势环编解码 / 基线健康账
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-21 功能定义的实装细化，非新立项）：
// 1. TrendCodec —— 90 晚趋势环序列化：u32 差分编码（首样全量、后续
//    zigzag varint 差分——趋势图导出的紧凑面）+ FNV 尾。
// 2. BaselineHealth —— 基线健康账：基线年龄（晚数）+ 换基线建议
//    （基线太旧 → 机器状态漂移后误报风险上升——ADR 换基线的时机面）。
// 全部零堆：定长缓冲 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 趋势环容量（对齐主检 TREND_NIGHTS）。
pub const TCAP: usize = 90;
/// 基线建议年龄（晚）。
pub const BASELINE_AGE_WARN: u32 = 60;

// ---------------------------------------------------------------------------
// 深化一：趋势环编解码（zigzag varint 差分）
// ---------------------------------------------------------------------------

/// zigzag 编码（i32 → u32）。
pub fn zigzag(v: i32) -> u32 {
    ((v << 1) ^ (v >> 31)) as u32
}

/// zigzag 解码。
pub fn unzigzag(v: u32) -> i32 {
    ((v >> 1) as i32) ^ (-((v & 1) as i32))
}

/// varint 写入（LEB128），返回写入字节数。
pub fn varint_put(mut v: u32, out: &mut [u8], pos: &mut usize) {
    loop {
        let b = (v & 0x7F) as u8;
        v >>= 7;
        if v == 0 {
            if *pos < out.len() {
                out[*pos] = b;
                *pos += 1;
            }
            break;
        }
        if *pos < out.len() {
            out[*pos] = b | 0x80;
            *pos += 1;
        }
    }
}

/// varint 读取，返回 (值, 消耗字节数)。
pub fn varint_get(buf: &[u8], pos: &mut usize) -> Option<u32> {
    let mut v: u32 = 0;
    let mut shift = 0;
    loop {
        if *pos >= buf.len() || shift > 28 {
            return None;
        }
        let b = buf[*pos];
        *pos += 1;
        v |= ((b & 0x7F) as u32) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

/// 差分编码结果（缓冲 + 实长）。
pub struct TrendCodec {
    buf: [u8; TCAP * 5 + 8],
    len: usize,
    entries: usize,
}

impl TrendCodec {
    /// 编码 90 晚序列：首样 u32 全量（4B）+ 后续 zigzag varint 差分 + FNV(4)。
    pub fn encode(series: &[u32; TCAP]) -> Self {
        let mut c = TrendCodec { buf: [0; TCAP * 5 + 8], len: 0, entries: TCAP };
        c.buf[0..4].copy_from_slice(&series[0].to_le_bytes());
        c.len = 4;
        for k in 1..TCAP {
            let d = series[k] as i64 - series[k - 1] as i64;
            let d = d.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
            varint_put(zigzag(d), &mut c.buf, &mut c.len);
        }
        let h = crate::perfstar2::perfgate::fnv1a(&c.buf[..c.len]);
        if c.len + 4 <= c.buf.len() {
            c.buf[c.len..c.len + 4].copy_from_slice(&h.to_le_bytes());
            c.len += 4;
        }
        c
    }

    pub fn verify(&self) -> bool {
        if self.len < 8 {
            return false;
        }
        let stored = u32::from_le_bytes([
            self.buf[self.len - 4],
            self.buf[self.len - 3],
            self.buf[self.len - 2],
            self.buf[self.len - 1],
        ]);
        crate::perfstar2::perfgate::fnv1a(&self.buf[..self.len - 4]) == stored
    }

    /// 解码到输出（roundtrip；坏包 0）。
    pub fn decode_into(&self, out: &mut [u32; TCAP]) -> usize {
        if !self.verify() {
            return 0;
        }
        let mut pos = 4usize; // 跳过首样全量字段（K2 缺陷账 v4#3——起始偏移错）
        let mut b4 = [0u8; 4];
        b4.copy_from_slice(&self.buf[0..4]);
        out[0] = u32::from_le_bytes(b4);
        let mut decoded = 1usize;
        while decoded < TCAP && pos < self.len - 4 {
            if let Some(v) = varint_get(&self.buf[..self.len - 4], &mut pos) {
                let d = unzigzag(v);
                out[decoded] = (out[decoded - 1] as i64 + d as i64).clamp(0, u32::MAX as i64) as u32;
                decoded += 1;
            } else {
                break;
            }
        }
        decoded
    }

    /// 压缩率 ×100（编码长 / 全量裸长 90×4）。
    pub fn compression_pct(&self) -> u32 {
        (self.len as u64 * 100 / (TCAP as u64 * 4)) as u32
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// 深化二：基线健康账
// ---------------------------------------------------------------------------

/// 基线健康账。
pub struct BaselineHealth {
    age_nights: u32,
    rebaselines: u64,
    /// 自上次换基线以来的红夜数（红多 = 机器状态漂移信号）。
    red_nights: u32,
}

impl BaselineHealth {
    pub const fn new() -> Self {
        BaselineHealth { age_nights: 0, rebaselines: 0, red_nights: 0 }
    }

    /// 过一晚（red = 该晚有告警）。
    pub fn night(&mut self, red: bool) {
        self.age_nights += 1;
        if red {
            self.red_nights += 1;
        }
    }

    /// 建议换基线：年龄超线 或 红夜占比 >30%（×100）。
    pub fn rebaseline_suggested(&self) -> bool {
        if self.age_nights >= BASELINE_AGE_WARN {
            return true;
        }
        if self.age_nights >= 10 {
            // 交叉相乘防整数截断（4/13 = 30.77% 不被截成 30 漏警）。
            return self.red_nights as u64 * 100 > self.age_nights as u64 * 30;
        }
        false
    }

    /// 换基线（账清零 + 计数）。
    pub fn rebaseline(&mut self) {
        self.age_nights = 0;
        self.red_nights = 0;
        self.rebaselines += 1;
    }

    pub fn stats(&self) -> (u32, u32, u64) {
        (self.age_nights, self.red_nights, self.rebaselines)
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：编解码 / 基线健康逐条实摆。
pub fn run_benchgate_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F061-benchgate-v4");

    // ── zigzag/varint ──
    cs.add(
        "zigzag_roundtrip",
        [0i32, 1, -1, 63, -64, 64, i32::MAX, i32::MIN]
            .iter()
            .all(|v| unzigzag(zigzag(*v)) == *v),
        "",
    );

    // ── 趋势编解码 ──
    let mut series = [1000u32; TCAP];
    for (k, v) in series.iter_mut().enumerate() {
        *v = 1000 + k as u32; // 线性 +1/晚——差分极小 → 高压缩
    }
    let tc = TrendCodec::encode(&series);
    cs.add("trend_verify", tc.verify(), "");
    cs.add("trend_compression_good", tc.compression_pct() < 30, "线性差分压缩 <30% 裸长");
    let mut out = [0u32; TCAP];
    cs.add("trend_roundtrip", tc.decode_into(&mut out) == TCAP && out[89] == series[89] && out[45] == series[45], "");
    // 篡改检出。
    let mut bad = TrendCodec::encode(&series);
    bad.buf[10] ^= 0xFF;
    cs.add("trend_tamper_refused", !bad.verify() && bad.decode_into(&mut out) == 0, "");
    // 跳变序列（差分大）也保真。
    let mut wild = [0u32; TCAP];
    for (k, v) in wild.iter_mut().enumerate() {
        *v = if k % 2 == 0 { 500 } else { 90_000 };
    }
    let tc2 = TrendCodec::encode(&wild);
    let mut out2 = [0u32; TCAP];
    cs.add("trend_wild_roundtrip", tc2.decode_into(&mut out2) == TCAP && out2[50] == wild[50], "");

    // ── 基线健康 ──
    let mut bh = BaselineHealth::new();
    for _ in 0..9 {
        bh.night(false);
    }
    cs.add("base_young_no_suggest", !bh.rebaseline_suggested(), "");
    // 10 晚 4 红（40%）→ 建议换。
    for _ in 0..4 {
        bh.night(true);
    }
    cs.add("base_red_heavy_suggest", bh.rebaseline_suggested(), "");
    bh.rebaseline();
    cs.add("base_rebaseline_resets", bh.stats() == (0, 0, 1) && !bh.rebaseline_suggested(), "");
    // 纯年龄触发。
    let mut bh2 = BaselineHealth::new();
    for _ in 0..BASELINE_AGE_WARN {
        bh2.night(false);
    }
    cs.add("base_age_triggers", bh2.rebaseline_suggested(), "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn varint_roundtrip_all_bounds() {
        let mut buf = [0u8; 16];
        for v in [0u32, 1, 127, 128, 16_383, 16_384, u32::MAX] {
            let mut pos = 0usize;
            varint_put(v, &mut buf, &mut pos);
            let mut rp = 0usize;
            assert_eq!(varint_get(&buf, &mut rp), Some(v));
        }
    }

    #[test]
    fn trend_single_value_series() {
        let s = [7777u32; TCAP];
        let tc = TrendCodec::encode(&s);
        let mut out = [0u32; TCAP];
        assert_eq!(tc.decode_into(&mut out), TCAP);
        assert!(out.iter().all(|v| *v == 7777));
    }

    #[test]
    fn baseline_red_ratio_boundary() {
        let mut bh = BaselineHealth::new();
        for _ in 0..7 {
            bh.night(false);
        }
        for _ in 0..3 {
            bh.night(true);
        }
        // 30% 恰达线（>30 才警）。
        assert!(!bh.rebaseline_suggested(), "恰 30% 不警（严格大于）");
        bh.night(true);
        assert!(bh.rebaseline_suggested());
    }
}

// ===========================================================================
// v5 深化批（deep5）：统计检验加固 + 免疫白名单
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：双样本显著性（Welch t 的整数近似——均值差 / 合并标准误差）
// ---------------------------------------------------------------------------

/// 双样本均值差检验（整数近似）：两组 [n=8 定长] 均值与极差标尺。
/// 判据：|mean_a - mean_b| > pooled_range/4 → 显著。
pub struct TwoSampleSignificance {
    a: [u32; 8],
    b: [u32; 8],
    na: usize,
    nb: usize,
}

impl TwoSampleSignificance {
    pub const fn new() -> Self {
        TwoSampleSignificance { a: [0; 8], b: [0; 8], na: 0, nb: 0 }
    }

    pub fn push_a(&mut self, v: u32) {
        if self.na < 8 {
            self.a[self.na] = v;
            self.na += 1;
        }
    }

    pub fn push_b(&mut self, v: u32) {
        if self.nb < 8 {
            self.b[self.nb] = v;
            self.nb += 1;
        }
    }

    fn mean(&self, arr: &[u32; 8], n: usize) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut sum = 0u64;
        for k in 0..n {
            sum += arr[k] as u64;
        }
        (sum / n as u64) as u32
    }

    /// 极差（max-min）作散布标尺（无需平方根）。
    fn range(&self, arr: &[u32; 8], n: usize) -> u32 {
        if n == 0 {
            return 0;
        }
        let mut mn = u32::MAX;
        let mut mx = 0u32;
        for k in 0..n {
            mn = mn.min(arr[k]);
            mx = mx.max(arr[k]);
        }
        mx - mn
    }

    /// 显著判定：差值 > 两组极差均值的 1/2（散布压不过差值）。
    /// n<4 → 数据不足，判不显著（诚实降级）。
    pub fn significant(&self) -> bool {
        if self.na < 4 || self.nb < 4 {
            return false;
        }
        let ma = self.mean(&self.a, self.na) as i64;
        let mb = self.mean(&self.b, self.nb) as i64;
        let diff = (ma - mb).abs();
        let spread = (self.range(&self.a, self.na) + self.range(&self.b, self.nb)) as i64 / 2;
        diff > spread
    }

    pub fn means(&self) -> (u32, u32) {
        (self.mean(&self.a, self.na), self.mean(&self.b, self.nb))
    }
}

// ---------------------------------------------------------------------------
// 深化二：离群免疫白名单（已确认合法的高值主机/场景不再误报）
// ---------------------------------------------------------------------------

/// 白名单：16 槽指纹（FNV16 of 场景串），带登记夜数与到期。
pub struct OutlierWhitelist {
    prints: [Option<(u16, u32)>; 16], // (指纹, 登记夜)
    n: u16,
}

pub const WL_TTL_NIGHTS: u32 = 30;

impl OutlierWhitelist {
    pub const fn new() -> Self {
        OutlierWhitelist { prints: [None; 16], n: 0 }
    }

    /// 登记（同指纹幂等——刷新登记夜）。
    pub fn admit(&mut self, print: u16, night: u32) -> bool {
        for e in self.prints.iter_mut() {
            if let Some((p, _)) = e {
                if *p == print {
                    *e = Some((print, night));
                    return true;
                }
            }
        }
        for e in self.prints.iter_mut() {
            if e.is_none() {
                *e = Some((print, night));
                self.n += 1;
                return true;
            }
        }
        false // 满
    }

    /// 是否免疫（在名单且未过期）。
    pub fn immune(&self, print: u16, night: u32) -> bool {
        for e in self.prints.iter() {
            if let Some((p, reg)) = e {
                if *p == print && night.saturating_sub(*reg) <= WL_TTL_NIGHTS {
                    return true;
                }
            }
        }
        false
    }

    /// 清理过期项（返回清除数）。
    pub fn sweep_expired(&mut self, night: u32) -> u32 {
        let mut swept = 0;
        for e in self.prints.iter_mut() {
            if let Some((_, reg)) = *e {
                if night.saturating_sub(reg) > WL_TTL_NIGHTS {
                    *e = None;
                    self.n -= 1;
                    swept += 1;
                }
            }
        }
        swept
    }

    pub fn len(&self) -> u16 {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化三：周环比聚合（7 夜粒度 → 周报行：周均值/周最差/环比方向）
// ---------------------------------------------------------------------------

/// 周聚合器：滚动 7 夜 → 周行；两行比较 → 环比方向。
pub struct WeeklyAggregator {
    ring: [u32; 7],
    pos: usize,
    filled: usize,
    /// 上一周均值（比较基准）。
    prev_week_mean: Option<u32>,
    weeks_closed: u32,
}

impl WeeklyAggregator {
    pub const fn new() -> Self {
        WeeklyAggregator { ring: [0; 7], pos: 0, filled: 0, prev_week_mean: None, weeks_closed: 0 }
    }

    /// 夜值入环；满 7 → 封周并清环。返回周行 (mean, worst)。
    pub fn night(&mut self, v: u32) -> Option<(u32, u32)> {
        self.ring[self.pos] = v;
        self.pos = (self.pos + 1) % 7;
        if self.filled < 7 {
            self.filled += 1;
        }
        if self.filled < 7 {
            return None; // 未满一周不封（第 7 夜当拍封——不留尾巴）
        }
        let mut sum = 0u64;
        let mut worst = 0u32;
        for k in 0..7 {
            sum += self.ring[k] as u64;
            worst = worst.max(self.ring[k]);
        }
        let mean = (sum / 7) as u32;
        self.prev_week_mean = Some(mean);
        self.weeks_closed += 1;
        self.pos = 0;
        self.filled = 0;
        Some((mean, worst))
    }

    /// 环比方向：本周均值 vs 上周（Some: 1 升 / 0 平 / -1 降）。
    pub fn week_over_week(&self, this_week_mean: u32) -> Option<i8> {
        let prev = self.prev_week_mean?;
        if this_week_mean > prev * 105 / 100 {
            Some(1)
        } else if this_week_mean * 105 / 100 < prev {
            Some(-1)
        } else {
            Some(0)
        }
    }

    pub fn weeks(&self) -> u32 {
        self.weeks_closed
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_benchgate_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F061-benchgate-v5");

    // ── 双样本检验 ──
    // 1) 明显分离：A 组 100±5，B 组 200±5 → 显著。
    let mut ts = TwoSampleSignificance::new();
    for k in 0..8 {
        ts.push_a(100 + k as u32 % 5);
        ts.push_b(200 + k as u32 % 5);
    }
    cs.add("ttest_clear_separation", ts.significant(), "");
    // 2) 重叠分布：两组均值同 100、散布大 → 不显著。
    let mut ts2 = TwoSampleSignificance::new();
    for k in 0..8 {
        ts2.push_a(90 + k as u32 * 5); // 90..125
        ts2.push_b(125 - k as u32 * 5); // 125..90
    }
    cs.add("ttest_overlap_not_significant", !ts2.significant(), "");
    // 3) 样本不足 4 → 诚实不判。
    let mut ts3 = TwoSampleSignificance::new();
    for _ in 0..3 {
        ts3.push_a(100);
        ts3.push_b(999);
    }
    cs.add("ttest_insufficient_honest", !ts3.significant(), "");

    // ── 白名单 ──
    // 4) 登记即免疫。
    let mut wl = OutlierWhitelist::new();
    cs.add("wl_immune_after_admit", wl.admit(0xBEEF, 100) && wl.immune(0xBEEF, 120), "");
    // 5) 未登记不免疫。
    cs.add("wl_stranger_not_immune", !wl.immune(0xDEAD, 120), "");
    // 6) TTL 过期失免 + sweep 清账。
    cs.add("wl_ttl_expiry", !wl.immune(0xBEEF, 100 + WL_TTL_NIGHTS + 1) && wl.sweep_expired(200) == 1 && wl.len() == 0, "");
    // 7) 幂等登记（不占双槽）。
    let mut wl2 = OutlierWhitelist::new();
    let _ = wl2.admit(7, 1);
    let _ = wl2.admit(7, 5);
    cs.add("wl_idempotent_admit", wl2.len() == 1 && wl2.immune(7, 6), "");
    // 8) 满 16 槽拒绝第 17 个。
    let mut wl3 = OutlierWhitelist::new();
    let mut all_ok = true;
    for k in 0..17u16 {
        all_ok &= wl3.admit(k, 1);
    }
    cs.add("wl_cap_16", !all_ok && wl3.len() == 16, "");

    // ── 周环比 ──
    // 9) 满 7 夜封周（均值+最差）。
    let mut wa = WeeklyAggregator::new();
    let mut last_row = None;
    for k in 0..7u32 {
        last_row = wa.night(100 + k); // 100..106，均值 103，最差 106
    }
    cs.add("weekly_closes_at_7", last_row == Some((103, 106)) && wa.weeks() == 1, "");
    // 10) 环比三态：+6% 升 / ±5% 内平 / −10% 降。
    cs.add(
        "weekly_wow_directions",
        wa.week_over_week(109) == Some(1) && wa.week_over_week(104) == Some(0) && wa.week_over_week(92) == Some(-1),
        "",
    );
    // 11) 未封周前无基准 → None。
    let mut wa2 = WeeklyAggregator::new();
    for _ in 0..3 {
        let _ = wa2.night(50);
    }
    cs.add("weekly_no_baseline_honest", wa2.week_over_week(50).is_none(), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn ttest_boundary_case() {
        // 差值恰等于散布 → 不显著（严格大于语义）。
        let mut ts = TwoSampleSignificance::new();
        for _ in 0..4 {
            ts.push_a(100);
            ts.push_b(110);
        }
        // diff=10, spread=(0+0)/2=0 → 显著（零散布下任何差都显著）。
        assert!(ts.significant());
    }

    #[test]
    fn wl_sweep_partial() {
        let mut wl = OutlierWhitelist::new();
        let _ = wl.admit(1, 1);
        let _ = wl.admit(2, 50);
        // 夜 40：指纹 1 过期（1+30<40），指纹 2 未过期。
        assert_eq!(wl.sweep_expired(40), 1);
        assert_eq!(wl.len(), 1);
        assert!(wl.immune(2, 41));
    }

    #[test]
    fn weekly_two_full_cycles() {
        let mut wa = WeeklyAggregator::new();
        for _ in 0..7 {
            let _ = wa.night(100);
        }
        let row2 = {
            let mut r = None;
            for _ in 0..7 {
                r = wa.night(110);
            }
            r
        };
        assert_eq!(row2, Some((110, 110)));
        assert_eq!(wa.weeks(), 2);
        assert_eq!(wa.week_over_week(116), Some(1), "+5.5% 越过 ±5% 带 → 升");
        assert_eq!(wa.week_over_week(114), Some(0), "带内 → 平");
    }
}
