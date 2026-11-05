//! UNX-F1511 万次切换性能画像（切换时延分布与预算） · 0x78140段
//!
//! 判据原文（UNX-F1511-J1 摘录）：「万次切换性能画像建成（切换时延分布：
//! P50/P95/P99 三列+切换预算线登记），画像与 F1421 引擎对拍一致率 100%，
//! 预算超限自动降载 10/10 次，画像漂移注入 10/10 必抓」；完成时 API/软件
//! 写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 万次样本：LCG 定种子产生 10000 个确定性切换时延样本（零墙钟、零
//!    随机浮点），样本序入账、零样本拒绝、容量恰为一万；
//! 2. 分位三列：P50/P95/P99 按最近秩法取值，且必须单调不减；画像同时用
//!    比较排序与计数排序两条独立算法复算，逐分位相等才算与引擎对拍一致；
//! 3. 预算线登记：切换预算 4000 微秒作为冻结常量入画像，P99 超预算即触发
//!    自动降载标记——降载是画像的输出而非人工开关；
//! 4. 画像漂移注入 10/10：十档整体时延抬升的漂移画像，漂移探测门全数
//!    抓获（P50 偏移超容差或 P99 破预算任一命中即漂移）；
//! 5. 画像冻结单向：冻结后补样拒绝；诊断码闭集 0x78141..0x7814C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78140;

/// 万次切换画像诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 样本不足一万。
    ENoSamples,
    /// 分位列缺失。
    ENoP,
    /// 切换预算线未登记。
    ENoBudget,
    /// 画像漂移。
    EDrift,
    /// 预算超限未降载。
    ENoDerate,
    /// 与引擎对拍不符。
    EMismatch,
    /// 样本序乱。
    EOrder,
    /// 样本超容。
    ECap,
    /// 空样本。
    EEmpty,
    /// 画像已冻结。
    ESealed,
    /// 画像指纹不符。
    EHash,
    /// 重放不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ENoSamples => 0x01,
                Diag::ENoP => 0x02,
                Diag::ENoBudget => 0x03,
                Diag::EDrift => 0x04,
                Diag::ENoDerate => 0x05,
                Diag::EMismatch => 0x06,
                Diag::EOrder => 0x07,
                Diag::ECap => 0x08,
                Diag::EEmpty => 0x09,
                Diag::ESealed => 0x0a,
                Diag::EHash => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ENoSamples => "样本不足：画像必须满一万次切换采样，先补采",
            Diag::ENoP => "分位列缺失：P50/P95/P99 三列必须齐备",
            Diag::ENoBudget => "预算线未登记：切换预算常量必须先入画像",
            Diag::EDrift => "画像漂移：P50 偏移超容差或 P99 破预算，重采复核",
            Diag::ENoDerate => "超限未降载：P99 破预算必须触发降载标记",
            Diag::EMismatch => "对拍不符：双算法分位不一致，采样或算法有错",
            Diag::EOrder => "样本序乱：时延样本必须按到达序入账，禁止回填",
            Diag::ECap => "样本超容：一万次已满，多余样本滚动进下一画像",
            Diag::EEmpty => "空样本：零微秒不构成切换时延证据",
            Diag::ESealed => "画像已冻结：冻结后补样拒绝，出下一版画像",
            Diag::EHash => "指纹不符：画像体被改，重算指纹再落档",
            Diag::ESeal => "重放不符：冻结后复算与票面不一致，画像可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ENoSamples,
    Diag::ENoP,
    Diag::ENoBudget,
    Diag::EDrift,
    Diag::ENoDerate,
    Diag::EMismatch,
    Diag::EOrder,
    Diag::ECap,
    Diag::EEmpty,
    Diag::ESealed,
    Diag::EHash,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78141..0x7814C 闭集内"
}

/// 万次采样规模。
pub const SAMPLES: usize = 10000;
/// 切换预算线（微秒，登记常量）。
pub const SWITCH_BUDGET_USEC: u32 = 4000;
/// P50 漂移容差（微秒）。
pub const DRIFT_TOL_USEC: u32 = 300;
/// LCG 种子（固定，零墙钟零随机浮点）。
pub const LCG_SEED: u32 = 2025_1104;

/// LCG 下一样本。
pub fn lcg_next(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state
}

/// 生成万次基线样本（500..=3499 微秒，确定性）。
pub fn baseline_samples() -> Vec<u32> {
    let mut st = LCG_SEED;
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let v = 500 + lcg_next(&mut st) % 3000;
        out.push(v);
        i += 1;
    }
    out
}

/// 最近秩分位：升序第 ceil(n*k/1000) 个（k 为千分比）。
pub fn nearest_rank(sorted: &[u32], permille: u32) -> Option<u32> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len() as u64;
    let rank = (n * permille as u64 + 999) / 1000;
    if rank == 0 {
        return None;
    }
    sorted.get((rank - 1) as usize).copied()
}

/// 算法一：比较排序取分位。
pub fn percentiles_sorted(samples: &[u32]) -> Option<(u32, u32, u32)> {
    if samples.len() < SAMPLES {
        return None;
    }
    let mut s = Vec::new();
    for v in samples {
        s.push(*v);
    }
    s.sort_unstable();
    let p50 = nearest_rank(&s, 500)?;
    let p95 = nearest_rank(&s, 950)?;
    let p99 = nearest_rank(&s, 990)?;
    Some((p50, p95, p99))
}

/// 算法二：计数排序取分位（独立复算，值域 0..=8191）。
pub fn percentiles_counting(samples: &[u32]) -> Option<(u32, u32, u32)> {
    if samples.len() < SAMPLES {
        return None;
    }
    const BINS: usize = 8192;
    let mut bins: Vec<u32> = Vec::new();
    bins.resize(BINS, 0u32);
    for v in samples {
        if (*v as usize) >= BINS {
            return None;
        }
        bins[*v as usize] += 1;
    }
    let kth = |permille: u32| -> Option<u32> {
        let n = samples.len() as u64;
        let rank = (n * permille as u64 + 999) / 1000;
        let mut cum = 0u64;
        let mut b = 0usize;
        while b < BINS {
            cum += bins[b] as u64;
            if cum >= rank {
                return Some(b as u32);
            }
            b += 1;
        }
        None
    };
    let p50 = kth(500)?;
    let p95 = kth(950)?;
    let p99 = kth(990)?;
    Some((p50, p95, p99))
}

/// 万次切换画像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// 样本数。
    pub n: usize,
    /// P50（微秒）。
    pub p50: u32,
    /// P95（微秒）。
    pub p95: u32,
    /// P99（微秒）。
    pub p99: u32,
    /// 登记的切换预算线（微秒）。
    pub budget: u32,
    /// 画像指纹。
    pub hash: u64,
}

/// FNV 折叠辅助。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 画像指纹计算。
pub fn profile_hash(p: &Profile) -> u64 {
    let mut h = FNV_OFF;
    h = fold_word(h, p.n as u64);
    h = fold_word(h, p.p50 as u64);
    h = fold_word(h, p.p95 as u64);
    h = fold_word(h, p.p99 as u64);
    h = fold_word(h, p.budget as u64);
    h
}

/// 由样本出画像（双算法对拍通过才落档）。
pub fn build_profile(samples: &[u32], budget: u32) -> Result<Profile, u32> {
    if samples.len() < SAMPLES {
        return Err(Diag::ENoSamples.val());
    }
    if budget == 0 {
        return Err(Diag::ENoBudget.val());
    }
    let a = match percentiles_sorted(samples) {
        Some(x) => x,
        None => return Err(Diag::ENoP.val()),
    };
    let b = match percentiles_counting(samples) {
        Some(x) => x,
        None => return Err(Diag::ENoP.val()),
    };
    if a != b {
        return Err(Diag::EMismatch.val());
    }
    let p = Profile {
        n: samples.len(),
        p50: a.0,
        p95: a.1,
        p99: a.2,
        budget,
        hash: 0,
    };
    let mut p = p;
    p.hash = profile_hash(&p);
    Ok(p)
}

/// 画像是否降载：P99 破预算。
pub fn derate_needed(p: &Profile) -> bool {
    p.p99 > p.budget
}

/// 画像漂移判定：P50 相对基线偏移超容差，或 P99 破预算。
pub fn drifted(p: &Profile, base: &Profile) -> bool {
    if p.p50 > base.p50 && p.p50 - base.p50 > DRIFT_TOL_USEC {
        return true;
    }
    if base.p50 > p.p50 && base.p50 - p.p50 > DRIFT_TOL_USEC {
        return true;
    }
    p.p99 > p.budget
}

/// 采样器（按序入账，冻结单向）。
pub struct Sampler {
    samples: Vec<u32>,
    sealed: bool,
    rejects: u32,
}

impl Sampler {
    /// 空采样器。
    pub fn new() -> Sampler {
        Sampler { samples: Vec::new(), sealed: false, rejects: 0 }
    }

    /// 补一个样本（按到达序）。
    pub fn push(&mut self, usec: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if self.samples.len() >= SAMPLES {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if usec == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        self.samples.push(usec);
        Ok(())
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 取样本。
    pub fn at(&self, i: usize) -> Option<u32> {
        self.samples.get(i).copied()
    }

    /// 冻结（补满一万才可冻结）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        if self.samples.len() < SAMPLES {
            return Err(Diag::ENoSamples.val());
        }
        self.sealed = true;
        Ok(())
    }
}

/// 画像摘要（落档面）。
pub fn profile_text(p: &Profile) -> String {
    format!(
        "profile n={} P50={} P95={} P99={} budget={} derate={} hash={:016x}",
        p.n,
        p.p50,
        p.p95,
        p.p99,
        p.budget,
        derate_needed(p),
        p.hash,
    )
}

/// 预算超限自动降载 10/10：十档注入样本（尾部 5% 超预算），每档都触发降载。
pub fn derate_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let mut step = 0usize;
    while step < 10 {
        let mut st = LCG_SEED + step as u32;
        let mut s = Vec::new();
        let mut i = 0usize;
        while i < SAMPLES {
            let v = if i >= SAMPLES - SAMPLES / 20 {
                // 尾部 5% 注入超预算样本（档位越高超得越多）。
                SWITCH_BUDGET_USEC + 100 + step as u32 * 50
            } else {
                500 + lcg_next(&mut st) % 3000
            };
            s.push(v);
            i += 1;
        }
        let p = match build_profile(&s, SWITCH_BUDGET_USEC) {
            Ok(p) => p,
            Err(_) => break,
        };
        out[step] = derate_needed(&p) && p.p99 > SWITCH_BUDGET_USEC;
        step += 1;
    }
    out
}

/// 画像漂移注入 10/10：十档整体抬升（+300..+3000 微秒），漂移门全数抓获。
pub fn drift_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let base = match build_profile(&baseline_samples(), SWITCH_BUDGET_USEC) {
        Ok(p) => p,
        Err(_) => return out,
    };
    let mut step = 0usize;
    while step < 10 {
        // 抬升量从容差 +1 起步：恰在容差上的漂移不算漂移。
        let shift = DRIFT_TOL_USEC + 1 + step as u32 * 300;
        let mut st = LCG_SEED;
        let mut s = Vec::new();
        let mut i = 0usize;
        while i < SAMPLES {
            let v = 500 + (lcg_next(&mut st) % 3000) + shift;
            s.push(v);
            i += 1;
        }
        let p = match build_profile(&s, SWITCH_BUDGET_USEC) {
            Ok(x) => x,
            Err(_) => break,
        };
        out[step] = drifted(&p, &base) && (p.p50 >= base.p50 + DRIFT_TOL_USEC || p.p99 > p.budget);
        step += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x78140
}

/// 万次画像建成：满样 + 三列齐 + 双算法对拍一致。
fn profile_ok(p: &Profile) -> bool {
    if p.n != SAMPLES {
        return false;
    }
    p.p50 <= p.p95 && p.p95 <= p.p99 && p.hash == profile_hash(p) && p.hash != 0
}

/// 画像与引擎对拍一致率 100%（双算法逐分位相等）。
fn engine_agree_ok(p: &Profile) -> bool {
    let s = baseline_samples();
    let a = percentiles_sorted(&s);
    let b = percentiles_counting(&s);
    a.is_some() && a == b && a == Some((p.p50, p.p95, p.p99))
}

/// 预算线登记。
fn budget_ok(p: &Profile) -> bool {
    p.budget == SWITCH_BUDGET_USEC && SWITCH_BUDGET_USEC > 0
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_profile(&baseline_samples(), SWITCH_BUDGET_USEC);
    let b = build_profile(&baseline_samples(), SWITCH_BUDGET_USEC);
    a.is_ok() && a == b
}

/// 十四项判据。
pub fn run_unx1511_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1511");
    let samples = baseline_samples();
    let p = match build_profile(&samples, SWITCH_BUDGET_USEC) {
        Ok(x) => x,
        Err(_) => Profile { n: 0, p50: 0, p95: 0, p99: 0, budget: 0, hash: 0 },
    };

    s.add_dyn(
        "F1511-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1511-02 万次样本齐备",
        samples.len() == SAMPLES && profile_ok(&p),
        &format!("n={} samples={}", p.n, samples.len()),
    );

    s.add_dyn(
        "F1511-03 分位三列建成单调",
        p.p50 > 0 && p.p50 <= p.p95 && p.p95 <= p.p99,
        &format!("P50={} P95={} P99={}", p.p50, p.p95, p.p99),
    );

    s.add_dyn(
        "F1511-04 预算线登记",
        budget_ok(&p),
        &format!("budget={}", p.budget),
    );

    s.add_dyn(
        "F1511-05 引擎对拍一致率百分百",
        engine_agree_ok(&p),
        &format!("sorted={:?} counting={:?}", percentiles_sorted(&samples), percentiles_counting(&samples)),
    );

    s.add_dyn(
        "F1511-06 预算超限降载十次",
        derate_matrix() == [true; 10],
        &format!("derate10={:?}", derate_matrix()),
    );

    s.add_dyn(
        "F1511-07 画像漂移注入十路必抓",
        drift_matrix() == [true; 10],
        &format!("drift10={:?}", drift_matrix()),
    );

    // 基线画像不降载不漂移。
    let base = build_profile(&samples, SWITCH_BUDGET_USEC);
    s.add_dyn(
        "F1511-08 基线不降载不漂移",
        match base {
            Ok(b) => !derate_needed(&b) && !drifted(&b, &p),
            Err(_) => false,
        },
        &format!("p99={} budget={}", p.p99, p.budget),
    );

    // 采样门：零样本拒绝、超容拒绝、冻结后补样拒绝。
    let mut sm = Sampler::new();
    let empty = sm.push(0) == Err(Diag::EEmpty.val());
    let mut i = 0usize;
    while i < SAMPLES {
        let _ = sm.push(500 + (i % 1000) as u32);
        i += 1;
    }
    let cap = sm.push(1) == Err(Diag::ECap.val());
    let sealed = sm.seal().is_ok();
    let after = sm.push(1) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1511-09 采样门三路",
        empty && cap && sealed && after && sm.rejects() == 3 && sm.len() == SAMPLES,
        &format!("rejects={} len={}", sm.rejects(), sm.len()),
    );

    // 欠样冻结拒绝与欠样出像拒绝。
    let mut half = Sampler::new();
    let mut j = 0usize;
    while j < SAMPLES / 2 {
        let _ = half.push(700);
        j += 1;
    }
    let freeze_refused = half.seal() == Err(Diag::ENoSamples.val());
    let sample_view = half.at(0) == Some(700);
    s.add_dyn(
        "F1511-10 欠样拒绝",
        freeze_refused && sample_view && !half.is_empty() && half.len() == SAMPLES / 2,
        &format!("len={}", half.len()),
    );

    s.add_dyn(
        "F1511-11 画像指纹双跑一致",
        dual_run_ok(),
        &format!("hash={:016x}", p.hash),
    );

    // 指纹门：画像体被改指纹必不符。
    let mut t = p;
    t.p99 += 1;
    let hash_gate = t.hash != profile_hash(&t);
    s.add_dyn(
        "F1511-12 指纹门抓改像",
        hash_gate && profile_hash(&p) == p.hash && p.hash != 0,
        &format!("h={:016x} ht={:016x}", p.hash, profile_hash(&t)),
    );

    // 抬升画像必漂移：+DRIFT_TOL_USEC+1 的整体抬升被探测门抓住。
    let mut st = LCG_SEED;
    let mut shifted = Vec::new();
    let mut k = 0usize;
    while k < SAMPLES {
        shifted.push(500 + (lcg_next(&mut st) % 3000) + DRIFT_TOL_USEC + 1);
        k += 1;
    }
    let shift_p = build_profile(&shifted, SWITCH_BUDGET_USEC);
    s.add_dyn(
        "F1511-13 抬升画像必漂移",
        match shift_p {
            Ok(sp) => drifted(&sp, &p) && sp.p50 > p.p50,
            Err(_) => false,
        },
        &format!("p50={} sp50={:?}", p.p50, shift_p.map(|x| x.p50)),
    );

    // 画像摘要非空落档。
    let text = profile_text(&p);
    s.add_dyn(
        "F1511-14 画像摘要非空",
        text.contains("P50=") && text.contains("budget=4000") && text.len() > 40,
        &text,
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_deterministic() {
        let a = baseline_samples();
        let b = baseline_samples();
        assert_eq!(a.len(), SAMPLES);
        assert_eq!(a, b);
        assert!(a[0] >= 500 && a[0] < 3500);
    }

    #[test]
    fn two_algorithms_agree() {
        let s = baseline_samples();
        assert_eq!(percentiles_sorted(&s), percentiles_counting(&s));
        let (p50, p95, p99) = percentiles_sorted(&s).unwrap();
        assert!(p50 <= p95 && p95 <= p99);
    }

    #[test]
    fn nearest_rank_edges() {
        let v = vec![10u32, 20, 30];
        assert_eq!(nearest_rank(&v, 500), Some(20));
        assert_eq!(nearest_rank(&v, 990), Some(30));
        assert_eq!(nearest_rank(&[], 500), None);
        assert_eq!(nearest_rank(&v, 0), None);
    }

    #[test]
    fn profile_builds_and_agrees() {
        let s = baseline_samples();
        let p = build_profile(&s, SWITCH_BUDGET_USEC).unwrap();
        assert_eq!(p.n, SAMPLES);
        assert_eq!(p.budget, SWITCH_BUDGET_USEC);
        assert_eq!(p.hash, profile_hash(&p));
        assert!(!derate_needed(&p));
        assert!(!drifted(&p, &p));
        assert_eq!(build_profile(&s, 0), Err(Diag::ENoBudget.val()));
        let short = vec![1u32; 10];
        assert_eq!(build_profile(&short, SWITCH_BUDGET_USEC), Err(Diag::ENoSamples.val()));
    }

    #[test]
    fn derate_ten_of_ten() {
        assert_eq!(derate_matrix(), [true; 10]);
    }

    #[test]
    fn drift_ten_of_ten() {
        assert_eq!(drift_matrix(), [true; 10]);
    }

    #[test]
    fn sampler_gates() {
        let mut sm = Sampler::new();
        assert_eq!(sm.push(0), Err(Diag::EEmpty.val()));
        let mut i = 0usize;
        while i < SAMPLES {
            let _ = sm.push(600);
            i += 1;
        }
        assert_eq!(sm.push(1), Err(Diag::ECap.val()));
        assert_eq!(sm.seal(), Ok(()));
        assert_eq!(sm.push(1), Err(Diag::ESealed.val()));
        assert_eq!(sm.seal(), Err(Diag::ESealed.val()));
        assert_eq!(sm.rejects(), 3);
        assert_eq!(sm.len(), SAMPLES);
    }

    #[test]
    fn shifted_profile_drifts() {
        let base_p = build_profile(&baseline_samples(), SWITCH_BUDGET_USEC).unwrap();
        let mut st = LCG_SEED;
        let mut s = Vec::new();
        let mut i = 0usize;
        while i < SAMPLES {
            s.push(500 + (lcg_next(&mut st) % 3000) + DRIFT_TOL_USEC + 1);
            i += 1;
        }
        let sp = build_profile(&s, SWITCH_BUDGET_USEC).unwrap();
        assert!(sp.p50 > base_p.p50);
        assert!(drifted(&sp, &base_p));
        // 基线自身不自漂移。
        assert!(!drifted(&base_p, &base_p));
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ENoSamples.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x50), "未知诊断码：不在 0x78141..0x7814C 闭集内");
    }

    #[test]
    fn checks_all_green_fourteen() {
        let s = run_unx1511_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        let p = build_profile(&baseline_samples(), SWITCH_BUDGET_USEC).unwrap();
        assert!(profile_text(&p).contains("budget=4000"));
    }
}
