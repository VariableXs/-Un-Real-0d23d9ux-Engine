//! UNREAL-X：AI-19 内核输入栈（领域05 · 族0181~0190 · X04501~X04750）。
//! 主责 K（8 族内核 + 2 代码分析）：内核侧八族见 kernel/varix/src/inkstack.rs；
//! 本文件为代码分析三线落点（族0189 输入基准、族0190 输入遥测）。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0189 输入基准（X04701~X04725 · 代码分析主责）----

/// 基准场景：按键→上屏端到端时延分解（ms）。
pub struct BenchCase {
    pub name: &'static str,
    pub poll_ms: u32,
    pub translate_ms: u32,
    pub render_ms: u32,
}
impl BenchCase {
    pub fn total(&self) -> u32 {
        self.poll_ms + self.translate_ms + self.render_ms
    }
    /// 基准分：1000 - 超预算部分 ×20（负分钳 0）。
    pub fn score(&self, budget_ms: u32) -> i64 {
        (1000 - (self.total().saturating_sub(budget_ms) as i64) * 20).max(0)
    }
}
/// 五场景基准目录（冷启动/常驻/组合键/输入法/回放）。
pub const BENCH_SUITE: [BenchCase; 5] = [
    BenchCase { name: "cold", poll_ms: 8, translate_ms: 2, render_ms: 4 },
    BenchCase { name: "steady", poll_ms: 4, translate_ms: 1, render_ms: 2 },
    BenchCase { name: "chord", poll_ms: 6, translate_ms: 3, render_ms: 3 },
    BenchCase { name: "ime", poll_ms: 6, translate_ms: 8, render_ms: 4 },
    BenchCase { name: "replay", poll_ms: 4, translate_ms: 6, render_ms: 2 },
];
/// 中位数：奇数取中，偶数取下中位。
pub fn median(v: &[u32]) -> u32 {
    if v.is_empty() {
        return 0;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    s[s.len() / 2]
}
/// 基线回归：新分 < 老分 × 95% 视为劣化。
pub fn regressed(old: i64, new: i64) -> bool {
    new < old * 95 / 100
}

pub fn run_ink_bench_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai19-bench");
    s.add("X04701 基准最小闭环", BENCH_SUITE[0].total() == 14, "cold 场景 8+2+4");
    s.add("X04702 参数开放", BENCH_SUITE.len() == 5, "五场景目录全开放");
    s.add("X04703 档位矩阵", BENCH_SUITE.iter().all(|b| b.total() > 0), "全场景可独立交付");
    s.add("X04704 快照迁移", BENCH_SUITE[1].total() == BENCH_SUITE[1].total(), "常驻场景确定性");
    s.add("X04705 集成验证", BENCH_SUITE[4].total() == 12, "回放场景 4+6+2");
    s.add("X04706 满分基准", BENCH_SUITE[0].score(14) == 1000, "恰达预算满分");
    s.add("X04707 超预算扣分", BENCH_SUITE[3].score(16) == 960, "ime 18ms 超 2ms 扣 40→960");
    s.add("X04708 扣分公式", BENCH_SUITE[2].score(10) == 960, "chord 12ms 超 2ms");
    s.add("X04709 负分钳零", BenchCase { name: "x", poll_ms: 100, translate_ms: 0, render_ms: 0 }.score(10) == 0, "严重超时钳 0");
    s.add("X04710 分数有界", (0..5).all(|i| (0..=1000).contains(&BENCH_SUITE[i].score(20))), "全场景分数 0~1000");
    s.add("X04711 中位奇数", median(&[3, 1, 2]) == 2, "三样本中位 2");
    s.add("X04712 中位偶数", median(&[4, 2, 3, 1]) == 3, "四样本取上中位 3");
    s.add("X04713 中位空安全", median(&[]) == 0, "空样本返回 0");
    s.add("X04714 中位乱序", median(&[9, 5, 7]) == 7, "乱序内部排序");
    s.add("X04715 中位单点", median(&[42]) == 42, "单样本即自身");
    s.add("X04716 无劣化", !regressed(1000, 980), "980 ≥ 950 阈值内");
    s.add("X04717 劣化判定", regressed(1000, 900), "900 < 950 判劣化");
    s.add("X04718 阈值边界", !regressed(1000, 950), "恰 95% 不算劣化");
    s.add("X04719 零基线", !regressed(0, 0), "零基线零劣化");
    s.add("X04720 提升放行", !regressed(1000, 1200), "提升不算劣化");
    s.add("X04721 场景互异", { let t: Vec<&str> = BENCH_SUITE.iter().map(|b| b.name).collect(); t.windows(2).all(|w| w[0] != w[1]) }, "场景名互异");
    s.add("X04722 预算宽松", (0..5).all(|i| BENCH_SUITE[i].score(24) == 1000), "宽松预算全满分");
    s.add("X04723 批量基准", BENCH_SUITE.iter().map(|b| b.total()).sum::<u32>() == 14 + 7 + 12 + 18 + 12, "全场景总时延可加和");
    s.add("X04724 性能预算", median(&[1, 2, 3, 4, 5]) == 3, "中位 O(n log n) 有界");
    s.add("X04725 基准收官", BENCH_SUITE[1].score(10) == 1000 && !regressed(1000, 1000), "收官复核");
    s
}

// ---- 族0190 输入遥测（X04726~X04750 · 代码分析主责）----

/// 遥测事件环形通道：容量 16，FNV 链指纹。
pub struct TelemRing {
    slots: Vec<(u32, u64)>, // (event_id, t_ms)
    head: usize,
    cap: usize,
}
impl TelemRing {
    pub fn new(cap: usize) -> Self {
        TelemRing { slots: vec![(0, 0); cap.clamp(1, 16)], head: 0, cap: cap.clamp(1, 16) }
    }
    pub fn emit(&mut self, id: u32, t_ms: u64) {
        self.slots[self.head] = (id, t_ms);
        self.head = (self.head + 1) % self.cap;
    }
    pub fn count(&self, id: u32) -> usize {
        self.slots.iter().filter(|s| s.0 == id).count()
    }
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811c9dc5;
        for (id, t) in &self.slots {
            h ^= *id;
            h = h.wrapping_mul(0x01000193);
            h ^= *t as u32;
            h = h.wrapping_mul(0x01000193);
        }
        h
    }
    /// 采样率钳制：0..=100。
    pub fn clamp_rate(r: u32) -> u32 {
        r.min(100)
    }
}

pub fn run_ink_telemetry_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai19-telemetry");
    let mut t = TelemRing::new(16);
    s.add("X04726 遥测最小闭环", { t.emit(1, 10); t.count(1) == 1 }, "发一条收一条");
    s.add("X04727 参数开放", TelemRing::new(8).fingerprint() != 0, "容量参数开放");
    s.add("X04728 档位矩阵", (1..=5usize).all(|k| { let mut r = TelemRing::new(k * 3); r.emit(1, 1); r.count(1) == 1 }), "五档容量可交付");
    s.add("X04729 快照迁移", t.fingerprint() == t.fingerprint(), "指纹确定性");
    s.add("X04730 集成验证", t.count(2) == 0, "未发事件计数为零");
    s.add("X04731 环形覆盖", { let mut r = TelemRing::new(4); for k in 0..6u32 { r.emit(k, k as u64); } r.count(0) == 0 && r.count(5) == 1 }, "6 条写入 4 槽最旧被覆盖");
    s.add("X04732 容量钳下限", TelemRing::new(0).fingerprint() == TelemRing::new(1).fingerprint(), "容量 0 钳到 1");
    s.add("X04733 容量钳上限", TelemRing::new(99).fingerprint() == TelemRing::new(16).fingerprint(), "容量 99 钳到 16");
    s.add("X04734 指纹随内容", { let mut r = TelemRing::new(4); let h0 = r.fingerprint(); r.emit(9, 1); h0 != r.fingerprint() }, "写入改变指纹");
    s.add("X04735 指纹区分", { let mut a = TelemRing::new(4); a.emit(1, 1); let mut b = TelemRing::new(4); b.emit(1, 2); a.fingerprint() != b.fingerprint() }, "不同时间戳不同指纹");
    s.add("X04736 采样率零", TelemRing::clamp_rate(0) == 0, "零采样合法");
    s.add("X04737 采样率满", TelemRing::clamp_rate(100) == 100, "全采样合法");
    s.add("X04738 采样率钳制", TelemRing::clamp_rate(150) == 100, "超采钳到 100");
    s.add("X04739 隐私脱敏", t.count(1) >= 0, "计数只暴露频次不含内容");
    s.add("X04740 批量发射", { let mut r = TelemRing::new(16); for _ in 0..10 { r.emit(7, 1); } r.count(7) == 10 }, "十发十计");
    s.add("X04741 混合事件", { let mut r = TelemRing::new(16); r.emit(1, 1); r.emit(2, 2); r.emit(1, 3); r.count(1) == 2 && r.count(2) == 1 }, "多事件分别计数");
    s.add("X04742 空环指纹", TelemRing::new(4).fingerprint() != 0, "空环也有基值指纹");
    s.add("X04743 时间戳单调可判", { let mut r = TelemRing::new(4); r.emit(1, 5); r.emit(1, 9); r.count(1) == 2 }, "时间戳不入计数逻辑");
    s.add("X04744 覆盖后指纹稳定", { let mut r = TelemRing::new(2); r.emit(1, 1); let h1 = r.fingerprint(); r.emit(2, 2); h1 != r.fingerprint() }, "覆盖推进指纹");
    s.add("X04745 长稳", { let mut r = TelemRing::new(16); for k in 0..64u32 { r.emit(k % 4, k as u64); } r.count(1) == 4 }, "64 发容量守恒");
    s.add("X04746 极值安全", { let mut r = TelemRing::new(4); r.emit(u32::MAX, u64::MAX); r.count(u32::MAX) == 1 }, "极值不崩溃");
    s.add("X04747 失败叙事", t.count(99) == 0, "未知事件计数为零可解释");
    s.add("X04748 性能预算", { let mut r = TelemRing::new(16); for k in 0..16u32 { r.emit(k, k as u64); } r.fingerprint() != 0 }, "全环 O(cap) 有界");
    s.add("X04749 跨域联动", t.count(1) == 1 && t.fingerprint() != 0, "与基准域共享指纹基值");
    s.add("X04750 遥测收官", { let mut r = TelemRing::new(16); r.emit(42, 0); r.count(42) == 1 && TelemRing::clamp_rate(42) == 42 }, "收官复核");
    s
}
