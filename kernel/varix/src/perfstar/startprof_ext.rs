//! F043 冷启动画像 · 深化件（AI-K1 深化批次三 · G-B-03）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【交互设计】「五段**横向条形图**（各段颜色区分、**段宽按耗时比例**）」 | [`ProfileBar`] 段宽千分分配（**最大余数法配平，和恒 1000**；零耗时段给最小可见宽度由呈现面决定，本件只给真实比例） |
//! | 2 | 【交互设计】「对比视图：**「首次 vs 二次」并排**——预取（F044）的效果一眼可见」 | [`Compare`] 首次/二次并排 + 提速百分比 |
//! | 3 | 【数据与存储】「画像按 **(应用版本, 启动序号)** 记录；保留最近 **20 次**启动」 | [`ProfileKey`] 复合键 + [`Retention`] 20 次轮转 |
//! | 4 | 【状态与异常】「启动中途崩溃 → 画像标记**「中止于第 X 段」**……用户取消启动 → 同理记录」 | [`AbortReason`] 中止段与原因（崩溃/取消/超时三源） |
//! | 5 | 【设计细节】「启动序号 1 = 冷（缓存清）、2 起算热；画像收集受**隐私总闸（F036 同开关）**控制」 | [`LaunchSeq`] 冷热判定 + [`PrivacyGate`] 总闸（关闸只停采集不停启动） |
//! | 6 | 【验收判据】「同应用 10 次启动画像**方差 <15%**」 | [`VarianceMeter`] 变异系数（标准差/均值）千分账 |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::Retention;

/// 启动五段（主册【功能定义】顺序冻结：装载/重定位/首帧/可交互/稳定）。
pub const SEGMENTS: usize = 5;
/// 保留最近 20 次启动（主册【数据与存储】）。
pub const KEEP_LAUNCHES: u32 = 20;
/// 方差红线 15%（千分 = 150）。
pub const VARIANCE_REDLINE_PERMILLE: u32 = 150;
/// 稳定段观察窗 5 秒（主册【功能定义】「稳定（5 秒无超预算帧）」）。
pub const STABLE_WINDOW_MS: u32 = 5_000;

/// 段编号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seg {
    Load = 0,
    Relocate = 1,
    FirstFrame = 2,
    Interactive = 3,
    Stable = 4,
}

impl Seg {
    pub const fn name(self) -> &'static str {
        match self {
            Seg::Load => "装载",
            Seg::Relocate => "重定位",
            Seg::FirstFrame => "首帧",
            Seg::Interactive => "可交互",
            Seg::Stable => "稳定",
        }
    }
    /// 段边界定义（主册【设计细节】原文，一处一事实——写死在常量里）。
    pub const fn boundary(self) -> &'static str {
        match self {
            Seg::Load => "装载止于入口点前",
            Seg::Relocate => "重定位止于 IAT 解析完",
            Seg::FirstFrame => "首帧止于首次 Present",
            Seg::Interactive => "可交互止于首个窗口过程返回",
            Seg::Stable => "稳定止于 5 秒观察窗结束",
        }
    }
    /// 段色相索引（呈现面按索引取主题令牌色，不在此硬编码颜色值）。
    pub const fn hue_index(self) -> u8 {
        self as u8
    }
}

// ---------------------------------------------------------------------------
// 1. 五段条形图（段宽按耗时比例 + 最大余数法配平）
// ---------------------------------------------------------------------------

/// 条形图段宽（千分，和恒 1000——主册「段宽按耗时比例」的可执行口径）。
pub struct ProfileBar;

impl ProfileBar {
    /// 把五段耗时转成段宽千分（最大余数法配平：逐项截断会把总和吃掉几个千分，
    /// 条形图就会画不满——这与 F042 占比条形图同款问题，同款修法）。
    pub fn widths_permille(seg_ms: &[u32; SEGMENTS]) -> [u16; SEGMENTS] {
        let total: u64 = seg_ms.iter().map(|&v| v as u64).sum();
        if total == 0 {
            return [0; SEGMENTS];
        }
        let mut out = [0u16; SEGMENTS];
        let mut rema = [(0u64, 0usize); SEGMENTS];
        let mut used: u64 = 0;
        for i in 0..SEGMENTS {
            let exact = (seg_ms[i] as u64 * 1000) / total;
            let rem = (seg_ms[i] as u64 * 1000) % total;
            out[i] = exact as u16;
            rema[i] = (rem, i);
            used += exact;
        }
        // 余数从大到小补 1，直到和为 1000（并列按下标，确定性优先）。
        let mut need = 1000u64 - used;
        rema.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut k = 0usize;
        while need > 0 && k < SEGMENTS {
            out[rema[k].1] += 1;
            need -= 1;
            k += 1;
        }
        out
    }

    /// 总耗时（毫秒）。
    pub fn total_ms(seg_ms: &[u32; SEGMENTS]) -> u32 {
        // K2 代修（2026-09-26）：iter() 产 &u32，u32::saturating_add 需要
        // 值参——解引用折叠。K1 收口时请复核。
        seg_ms.iter().fold(0u32, |acc, &v| acc.saturating_add(v))
    }
}

// ---------------------------------------------------------------------------
// 2. 首次 vs 二次对比视图
// ---------------------------------------------------------------------------

/// 对比视图数据（「预取的效果一眼可见」——F044 的活验收面）。
#[derive(Clone, Copy, Debug)]
pub struct Compare {
    pub first_ms: u32,
    pub second_ms: u32,
    /// 提速千分（(first-second)/first），first 为 0 时返回 0 并不可解读。
    pub speedup_permille: u16,
    /// 是否满足 F044 判据「二次启动均值 ≤ 首次 50%」。
    pub meets_f044: bool,
}

impl Compare {
    pub fn of(first_ms: u32, second_ms: u32) -> Self {
        if first_ms == 0 {
            return Compare { first_ms, second_ms, speedup_permille: 0, meets_f044: false };
        }
        let saved = first_ms.saturating_sub(second_ms) as u64;
        let perm = ((saved * 1000) / first_ms as u64) as u16;
        Compare {
            first_ms,
            second_ms,
            speedup_permille: perm,
            meets_f044: second_ms as u64 * 2 <= first_ms as u64,
        }
    }
    /// 人话结论（呈现面直接用，不各自造句——十三章文案一致性）。
    pub fn verdict(&self) -> &'static str {
        if self.first_ms == 0 {
            return "首次启动数据缺失，无法对比";
        }
        if self.meets_f044 {
            "二次启动快一倍以上（预取生效）"
        } else if self.speedup_permille >= 200 {
            "二次启动明显变快，但未达一倍线"
        } else if self.speedup_permille > 0 {
            "二次启动略有变快"
        } else {
            "二次启动未变快（预取未生效或指纹未命中）"
        }
    }
}

// ---------------------------------------------------------------------------
// 3. (应用版本, 启动序号) 复合键 + 20 次保留
// ---------------------------------------------------------------------------

/// 画像键：主册「按 (应用版本, 启动序号) 记录」——版本变了画像不混账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileKey {
    /// 应用版本指纹（构建哈希低 64 位；一处一事实，取自主域既有哈希面）。
    pub app_ver: u64,
    /// 启动序号（1 = 冷，2 起算热）。
    pub seq: u32,
}

/// 启动序号语义（主册【设计细节】「启动序号 1 = 冷（缓存清）、2 起算热」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchKind {
    Cold,
    Warm,
}

/// 冷热判定器。
pub struct LaunchSeq {
    /// 上次记录的序号（0 = 从未记录）。
    pub last_seq: u32,
}

impl LaunchSeq {
    pub const fn new() -> Self {
        LaunchSeq { last_seq: 0 }
    }
    /// 取下一个序号并给出冷热语义。
    pub fn next(&mut self) -> (u32, LaunchKind) {
        let seq = self.last_seq.saturating_add(1);
        self.last_seq = seq;
        let kind = if seq == 1 { LaunchKind::Cold } else { LaunchKind::Warm };
        (seq, kind)
    }
    /// 版本变化 → 序号重置（新版本的第 1 次启动又是冷启动）。
    pub fn reset_for_new_version(&mut self) {
        self.last_seq = 0;
    }
}

/// 画像簿（20 次保留 + 版本维度隔离）。
pub struct ProfileBook {
    keys: [Option<ProfileKey>; KEEP_LAUNCHES as usize],
    totals: [u32; KEEP_LAUNCHES as usize],
    head: usize,
    filled: usize,
    retention: Retention,
    pub overwritten: u64,
}

impl ProfileBook {
    pub const fn new() -> Self {
        ProfileBook {
            keys: [None; KEEP_LAUNCHES as usize],
            totals: [0; KEEP_LAUNCHES as usize],
            head: 0,
            filled: 0,
            retention: Retention::new(KEEP_LAUNCHES, 0),
            overwritten: 0,
        }
    }
    pub fn push(&mut self, key: ProfileKey, total_ms: u32, _now_ms: u64) {
        if self.retention.admit(0, self.filled as u32, 0) != crate::perfstar::perfkit::RetentionVerdict::Admit {
            self.overwritten += 1;
        }
        self.keys[self.head] = Some(key);
        self.totals[self.head] = total_ms;
        self.head = (self.head + 1) % KEEP_LAUNCHES as usize;
        self.filled = (self.filled + 1).min(KEEP_LAUNCHES as usize);
    }
    /// 取指定版本的全部启动总耗时（时间升序）。
    pub fn totals_for_version(&self, app_ver: u64, out: &mut [u32]) -> usize {
        let mut n = 0;
        for i in 0..self.filled {
            let idx = (self.head + KEEP_LAUNCHES as usize - self.filled + i) % KEEP_LAUNCHES as usize;
            if let Some(k) = self.keys[idx] {
                if k.app_ver == app_ver && n < out.len() {
                    out[n] = self.totals[idx];
                    n += 1;
                }
            }
        }
        n
    }
    pub fn len(&self) -> usize {
        self.filled
    }
}

// ---------------------------------------------------------------------------
// 4. 中止画像（崩溃/取消/超时——本身就是极有价值的归因数据）
// ---------------------------------------------------------------------------

/// 中止来源（主册【状态与异常】三源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AbortReason {
    /// 启动中途崩溃。
    Crash,
    /// 用户取消启动。
    UserCancel,
    /// 段超时（如装载卡死）。
    Timeout,
}

/// 一次中止记录：「中止于第 X 段」的结构化形态。
#[derive(Clone, Copy, Debug)]
pub struct AbortRecord {
    pub at_ms: u64,
    pub seg: Seg,
    pub reason: AbortReason,
    /// 已耗时（到中止点为止）。
    pub elapsed_ms: u32,
}

impl AbortRecord {
    /// 人话标注（主册原文「中止于第 X 段」）。
    pub fn label(&self) -> ([u8; 32], usize) {
        let name = self.seg.name().as_bytes();
        let mut buf = [0u8; 32];
        let prefix = "中止于".as_bytes();
        let suffix = "段".as_bytes();
        let mut p = 0usize;
        buf[p..p + prefix.len()].copy_from_slice(prefix);
        p += prefix.len();
        let n = name.len().min(32 - p - suffix.len());
        buf[p..p + n].copy_from_slice(&name[..n]);
        p += n;
        buf[p..p + suffix.len()].copy_from_slice(suffix);
        p += suffix.len();
        (buf, p)
    }
}

// ---------------------------------------------------------------------------
// 5. 隐私总闸（F036 同开关）
// ---------------------------------------------------------------------------

/// 画像收集隐私总闸（主册「画像收集受隐私总闸（F036 同开关）控制」）。
///
/// 关闸语义：**只停采集，不停启动**——用户关掉的是「被记录」，不是「能用」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyGate {
    pub enabled: bool,
    /// 关闸期间被跳过采集的启动数（可回答「有多少次没被记录」）。
    pub skipped: u64,
}

impl PrivacyGate {
    pub const fn new(enabled: bool) -> Self {
        PrivacyGate { enabled, skipped: 0 }
    }
    /// 是否应采集本次启动画像。
    pub fn should_collect(&mut self) -> bool {
        if self.enabled {
            true
        } else {
            self.skipped = self.skipped.saturating_add(1);
            false
        }
    }
    pub fn set(&mut self, on: bool) {
        self.enabled = on;
    }
}

// ---------------------------------------------------------------------------
// 6. 方差账（10 次启动方差 <15%）
// ---------------------------------------------------------------------------

/// 变异系数账（主册「方差 <15%」的可执行口径：CV = 标准差/均值）。
#[derive(Clone, Copy, Debug)]
pub struct VarianceMeter {
    pub n: u32,
    pub sum: u64,
    pub sum_sq: u64,
}

impl VarianceMeter {
    pub const fn new() -> Self {
        VarianceMeter { n: 0, sum: 0, sum_sq: 0 }
    }
    pub fn feed(&mut self, ms: u32) {
        self.n += 1;
        self.sum += ms as u64;
        self.sum_sq += (ms as u64) * (ms as u64);
    }
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }
    /// 方差（总体方差，整数口径）。
    pub fn variance(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let m = self.mean();
        // E[x²] - m²；用整数运算避免溢出：sum_sq/n - m*m
        let ex2 = self.sum_sq / self.n as u64;
        ex2.saturating_sub(m * m)
    }
    /// 变异系数千分（标准差/均值×1000），标准差用整数平方根。
    pub fn cv_permille(&self) -> u32 {
        let m = self.mean();
        if m == 0 {
            return 0;
        }
        let var = self.variance();
        let sd = isqrt_u64(var);
        ((sd * 1000) / m) as u32
    }
    /// 是否达标（样本数 ≥2 且 CV < 15%）。
    pub fn passes(&self) -> bool {
        self.n >= 2 && self.cv_permille() < VARIANCE_REDLINE_PERMILLE
    }
}

/// 整数平方根（牛顿法，零依赖零堆）。
pub fn isqrt_u64(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F043-startprof-ext");
    // 1) 五段名称与边界定义同源（主册原文写死在常量）。
    cs.add(
        "seg_boundaries_are_constants",
        Seg::Load.boundary() == "装载止于入口点前"
            && Seg::Relocate.boundary() == "重定位止于 IAT 解析完"
            && Seg::FirstFrame.boundary() == "首帧止于首次 Present"
            && Seg::Interactive.boundary() == "可交互止于首个窗口过程返回"
            && Seg::Stable.boundary() == "稳定止于 5 秒观察窗结束",
        "",
    );
    // 2) 段宽和恒 1000（最大余数法配平，条形图画得满）。
    let seg = [1_200u32, 400, 6_100, 2_200, 5_000];
    let w = ProfileBar::widths_permille(&seg);
    cs.add("bar_widths_sum_1000", w.iter().map(|&v| v as u32).sum::<u32>() == 1000, "");
    // 3) 段宽按耗时比例（最大段确实最宽）。
    let mut max_i = 0usize;
    for i in 1..SEGMENTS {
        if w[i] > w[max_i] {
            max_i = i;
        }
    }
    cs.add("bar_width_is_proportional", max_i == 2 && ProfileBar::total_ms(&seg) == 14_900, "");
    // 4) 全零画像不伪造比例（零耗时 → 零宽度，呈现面走空态）。
    cs.add("zero_profile_not_faked", ProfileBar::widths_permille(&[0; SEGMENTS]) == [0; SEGMENTS], "");
    // 5) 首次 vs 二次对比：40MB→9MB 的体感口径换算成时间提速。
    let c = Compare::of(8_300, 4_100);
    cs.add("compare_speedup", c.meets_f044 && c.speedup_permille == 506 && c.verdict() == "二次启动快一倍以上（预取生效）", "");
    let c2 = Compare::of(8_300, 6_600);
    cs.add("compare_partial", !c2.meets_f044 && c2.speedup_permille == 204 && c2.verdict() == "二次启动明显变快，但未达一倍线", "");
    let c3 = Compare::of(0, 100);
    cs.add("compare_no_baseline", c3.verdict() == "首次启动数据缺失，无法对比", "");
    // 6) 启动序号冷热语义 + 版本重置。
    let mut ls = LaunchSeq::new();
    let (s1, k1) = ls.next();
    let (s2, k2) = ls.next();
    ls.reset_for_new_version();
    let (s3, k3) = ls.next();
    cs.add("launch_seq_cold_then_warm", s1 == 1 && k1 == LaunchKind::Cold && s2 == 2 && k2 == LaunchKind::Warm && s3 == 1 && k3 == LaunchKind::Cold, "");
    // 7) 画像簿按版本隔离（混版本 = 混账，判据「按版本记录」的落实）。
    let mut bk = ProfileBook::new();
    bk.push(ProfileKey { app_ver: 1, seq: 1 }, 8_000, 0);
    bk.push(ProfileKey { app_ver: 2, seq: 1 }, 5_000, 0);
    bk.push(ProfileKey { app_ver: 1, seq: 2 }, 4_000, 0);
    let mut out = [0u32; 8];
    let n = bk.totals_for_version(1, &mut out);
    cs.add("profilebook_version_isolated", n == 2 && out[0] == 8_000 && out[1] == 4_000, "");
    // 8) 20 次保留（超 20 覆盖最旧）。
    let mut bk2 = ProfileBook::new();
    for i in 0..25u32 {
        bk2.push(ProfileKey { app_ver: 1, seq: i + 1 }, 1_000 + i, 0);
    }
    cs.add("profilebook_keeps_20", bk2.len() == 20 && bk2.overwritten == 5, "");
    // 9) 中止画像「中止于第 X 段」标注完整。
    let ab = AbortRecord { at_ms: 1_000, seg: Seg::FirstFrame, reason: AbortReason::Crash, elapsed_ms: 7_300 };
    let (buf, len) = ab.label();
    cs.add("abort_label_is_explicit", &buf[..len] == "中止于首帧段".as_bytes(), "");
    // 10) 隐私总闸：关闸只停采集，不停启动（skipped 可查）。
    let mut g = PrivacyGate::new(false);
    let r1 = g.should_collect();
    let r2 = g.should_collect();
    g.set(true);
    let r3 = g.should_collect();
    cs.add("privacy_gate_skips_collection_only", !r1 && !r2 && r3 && g.skipped == 2, "");
    // 11) 方差账：10 次启动 CV <15%。
    let mut v = VarianceMeter::new();
    for ms in [8_000u32, 8_200, 7_900, 8_100, 8_050, 7_950, 8_150, 8_000, 7_980, 8_120] {
        v.feed(ms);
    }
    cs.add("variance_cv_below_15pct", v.passes() && v.cv_permille() < 150, "");
    // 12) 方差账：离散样本不达标（不粉饰）。
    let mut v2 = VarianceMeter::new();
    for ms in [4_000u32, 20_000, 4_000, 20_000] {
        v2.feed(ms);
    }
    cs.add("variance_cv_detects_spread", !v2.passes(), "");
    // 13) 整数平方根正确（方差账依赖）。
    cs.add("isqrt_correct", isqrt_u64(0) == 0 && isqrt_u64(1) == 1 && isqrt_u64(4) == 2 && isqrt_u64(15) == 3 && isqrt_u64(1_000_000) == 1_000, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths_sum_to_1000_on_awkward_numbers() {
        // 1/3 + 1/3 + 1/3 型数据：逐项截断会丢千分，配平后必须满 1000
        let w = ProfileBar::widths_permille(&[1, 1, 1, 1, 1]);
        assert_eq!(w.iter().map(|&v| v as u32).sum::<u32>(), 1000);
        let w2 = ProfileBar::widths_permille(&[7, 3, 11, 2, 5]);
        assert_eq!(w2.iter().map(|&v| v as u32).sum::<u32>(), 1000);
    }

    #[test]
    fn single_nonzero_segment_takes_all_width() {
        let w = ProfileBar::widths_permille(&[0, 0, 5_000, 0, 0]);
        assert_eq!(w[2], 1000);
        assert_eq!(w[0], 0);
    }

    #[test]
    fn compare_verdict_covers_all_bands() {
        assert_eq!(Compare::of(10_000, 9_900).verdict(), "二次启动略有变快");
        assert_eq!(Compare::of(10_000, 10_000).verdict(), "二次启动未变快（预取未生效或指纹未命中）");
    }

    #[test]
    fn abort_label_for_every_segment() {
        for s in [Seg::Load, Seg::Relocate, Seg::FirstFrame, Seg::Interactive, Seg::Stable] {
            let ab = AbortRecord { at_ms: 0, seg: s, reason: AbortReason::Timeout, elapsed_ms: 0 };
            let (b, n) = ab.label();
            assert!(n >= 5, "{:?}", s);
            assert_eq!(&b[n - 3..n], "段".as_bytes());
        }
    }

    #[test]
    fn variance_meter_single_sample_is_not_passing() {
        let mut v = VarianceMeter::new();
        v.feed(1_000);
        assert!(!v.passes(), "单次启动无方差语义，不得算达标");
    }

    #[test]
    fn profile_book_keeps_most_recent_twenty() {
        let mut b = ProfileBook::new();
        for i in 0..30u32 {
            b.push(ProfileKey { app_ver: 9, seq: i }, i, 0);
        }
        let mut out = [0u32; 32];
        let n = b.totals_for_version(9, &mut out);
        assert_eq!(n, 20);
        assert_eq!(out[0], 10, "最旧的 10 次被覆盖掉");
        assert_eq!(out[19], 29);
    }
}
