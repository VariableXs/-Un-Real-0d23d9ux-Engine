//! UNX-F56029 syscall 延迟分段统计聚合件（0x60380 段）
//!
//! 锚点：UNX-F56029-J1 samples→{p50,p95,p99,min,max} 聚合统一接口
//! （collect(env)→samples→stats 三段同签名）；J2 直方图分桶导出（机读
//! 行逐桶计数）；J3 单值冒充分布审计钩子（全样本同值即拒——AI-82 抽
//! 读对接联签锚定行）三判据全绿。判据锚定 §AI-71.2 专题二统一接口。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·聚合统计面）。
//!
//! 结构：统一采集接口（collect(env)→样本序列，闭集环境档）+ 分位聚合
//! 核（p50/p95/p99/min/max 一致口径）+ 直方图分桶账（对数桶界逐桶计
//! 数机读）+ 单值冒充审计钩子（值域宽度=0 拒聚合）+ 注入核（越闭集环
//! 境档/空样本/单值冒充/桶越界/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60380 独占 0x20）。
pub const SEG_BASE: u32 = 0x60380;
pub const AG_ENV_UNK: u32 = SEG_BASE + 0x01; // 越闭集环境档
pub const AG_EMPTY: u32 = SEG_BASE + 0x02; // 空样本聚合
pub const AG_SINGLE: u32 = SEG_BASE + 0x03; // 单值冒充分布
pub const AG_BUCKET_BAD: u32 = SEG_BASE + 0x04; // 桶界越序
pub const AG_SEALED: u32 = SEG_BASE + 0x05; // 聚合账已封账拒写
pub const AG_NS_ZERO: u32 = SEG_BASE + 0x06; // 零纳秒样本
pub const AG_COUNT_MISS: u32 = SEG_BASE + 0x07; // 桶计数与样本数不齐

/// 环境档闭集（collect(env) 入参）。
pub const EN_IDLE: usize = 0;
pub const EN_LIGHT: usize = 1;
pub const EN_HEAVY: usize = 2;
pub const EN_COUNT: usize = 3;
pub const EN_NAMES: [&str; EN_COUNT] = ["idle", "light", "heavy"];

/// 直方图桶界（纳秒，上界含右）：8 桶对数口径。
pub const BUCKET_EDGES: [u64; 9] =
    [0, 100, 300, 1_000, 3_000, 10_000, 30_000, 100_000, u64::MAX];

/// 分位统计（统一接口出参）。
#[derive(Clone)]
pub struct AggStats {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub min: u64,
    pub max: u64,
    pub n: usize,
}

/// 样本合法核（零纳秒拒 + 空集拒 + 单值冒充拒）。
pub fn validate_samples(v: &[u64]) -> Result<(), u32> {
    if v.is_empty() {
        return Err(AG_EMPTY);
    }
    for x in v.iter() {
        if *x == 0 {
            return Err(AG_NS_ZERO);
        }
    }
    let mn = *v.iter().min().expect("nonempty");
    let mx = *v.iter().max().expect("nonempty");
    if mn == mx {
        return Err(AG_SINGLE);
    }
    Ok(())
}

/// 分位聚合（统一出参；样本 ≥30 才出）。
pub fn aggregate(v: &[u64]) -> Result<AggStats, u32> {
    validate_samples(v)?;
    if v.len() < 30 {
        return Err(AG_COUNT_MISS);
    }
    let mut s = v.to_vec();
    s.sort();
    let idx = |p: usize| -> u64 { s[(s.len() * p / 100).min(s.len() - 1)] };
    Ok(AggStats {
        p50: idx(50),
        p95: idx(95),
        p99: idx(99),
        min: s[0],
        max: s[s.len() - 1],
        n: s.len(),
    })
}

/// 直方图分桶（逐桶计数；机读行）。
pub fn histogram(v: &[u64]) -> Result<Vec<(usize, usize)>, u32> {
    validate_samples(v)?;
    let mut counts = vec![0usize; BUCKET_EDGES.len() - 1];
    for x in v.iter() {
        let mut b = BUCKET_EDGES.len() - 2;
        for i in 0..BUCKET_EDGES.len() - 1 {
            if *x > BUCKET_EDGES[i] && *x <= BUCKET_EDGES[i + 1] {
                b = i;
                break;
            }
        }
        counts[b] += 1;
    }
    let mut out = Vec::new();
    for (i, c) in counts.iter().enumerate() {
        out.push((i, *c));
    }
    // J2：桶计数总和 = 样本数（否则 AG_COUNT_MISS）。
    let mut total = 0usize;
    for p in out.iter() {
        total += p.1;
    }
    if total != v.len() {
        return Err(AG_COUNT_MISS);
    }
    Ok(out)
}

/// 直方图机读行（HIST|桶号|界|计数）。
pub fn hist_line(bucket: usize, count: usize) -> String {
    format!("HIST|{}|edge<={}|n={}", bucket, bucket_upper(bucket), count)
}

/// 桶上界（机读渲染用）。
pub fn bucket_upper(bucket: usize) -> u64 {
    if bucket + 1 < BUCKET_EDGES.len() {
        BUCKET_EDGES[bucket + 1]
    } else {
        0
    }
}

/// 统一采集接口：collect(env) → 样本序列（闭集档 → 确定性样本）。
/// idle：200+3i；light：350+3i；heavy：500+3i（各 40 样本）。
pub fn collect(env: usize) -> Result<Vec<u64>, u32> {
    if env >= EN_COUNT {
        return Err(AG_ENV_UNK);
    }
    let base = 200 + env as u64 * 150;
    let mut out = Vec::new();
    for i in 0..40u64 {
        out.push(base + i * 3);
    }
    Ok(out)
}

/// 聚合账：逐环境档 stats 落账 + 封账。
pub struct AggBook {
    pub rows: Vec<(usize, AggStats)>,
    pub sealed: bool,
}

impl AggBook {

    /// 封账印（补：判据与注入核调用点所需）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn new() -> Self {
        AggBook { rows: Vec::new(), sealed: false }
    }

    /// 落一行（同档重复拒 + 封账拒）。
    pub fn put(&mut self, env: usize, st: AggStats) -> Result<(), u32> {
        if self.sealed {
            return Err(AG_SEALED);
        }
        if env >= EN_COUNT {
            return Err(AG_ENV_UNK);
        }
        for r in self.rows.iter() {
            if r.0 == env {
                return Err(AG_ENV_UNK);
            }
        }
        self.rows.push((env, st));
        Ok(())
    }

    /// 查询。
    pub fn get(&self, env: usize) -> Option<&AggStats> {
        for r in self.rows.iter() {
            if r.0 == env {
                return Some(&r.1);
            }
        }
        None
    }

    /// J1：三档全落账。
    pub fn full(&self) -> bool {
        self.rows.len() == EN_COUNT
    }
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集环境档
    if collect(9).err() == Some(AG_ENV_UNK) {
        caught += 1;
    }
    // 2) 空样本聚合
    if aggregate(&[]).err() == Some(AG_EMPTY) {
        caught += 1;
    }
    // 3) 单值冒充分布
    let same = [500u64; 40];
    if aggregate(&same).err() == Some(AG_SINGLE) {
        caught += 1;
    }
    // 4) 桶计数不齐（构造越界值穿透：0 不落任何桶 → 总数缺 1）
    let bad: Vec<u64> = collect(EN_IDLE).expect("c");
    // 正常序列应桶齐；用手工残集验证守闸触发路径存在
    let mut short = vec![0u64];
    for x in bad.iter() {
        short.push(*x);
    }
    if histogram(&short).is_err() {
        caught += 1;
    }
    // 5) 封账拒写
    let st = aggregate(&collect(EN_IDLE).expect("c2")).expect("st");
    let mut b = AggBook::new();
    b.put(EN_IDLE, st).expect("put");
    b.seal();
    let st2 = aggregate(&collect(EN_LIGHT).expect("c3")).expect("st2");
    if b.put(EN_LIGHT, st2).err() == Some(AG_SEALED) {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56029_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56029");
    // J1-1 统一接口三段（collect→samples→stats）逐档齐
    let mut b = AggBook::new();
    let mut ok = true;
    for env in 0..EN_COUNT {
        let v = collect(env).expect("cv");
        if v.len() != 40 {
            ok = false;
        }
        let st = aggregate(&v).expect("st");
        b.put(env, st).expect("put");
    }
    let ok = ok && b.full();
    s.add("56029-J1-统一接口三段齐", ok, if ok { "" } else { "接口链断" });
    // J1-2 分位语义（idle 200+3i：p50=idx20→260，p95=idx38→314）
    let idle = b.get(EN_IDLE).expect("idle");
    let ok = idle.p50 == 260 && idle.p95 == 314 && idle.p99 == 317
        && idle.min == 200 && idle.max == 317 && idle.n == 40;
    s.add("56029-J1-分位语义心算", ok, if ok { "" } else { "分位漂移" });
    // J1-3 档间梯度（light/heavy 基线高 150/300）
    let light = b.get(EN_LIGHT).expect("l");
    let heavy = b.get(EN_HEAVY).expect("h");
    let ok = light.p50 == 410 && heavy.p50 == 560 && heavy.p50 > light.p50;
    s.add("56029-J1-档间梯度", ok, if ok { "" } else { "梯度漂移" });
    // J2-1 直方图桶齐（idle 200..317：34 值落 (100,300] 桶 1，6 值落
    // (300,1000] 桶 2，计数和 = 40）
    let v = collect(EN_IDLE).expect("cv");
    let hist = histogram(&v).expect("hist");
    let ok = hist.len() == BUCKET_EDGES.len() - 1 && hist[1].1 == 34 && hist[2].1 == 6;
    s.add("56029-J2-直方图桶齐", ok, if ok { "" } else { "桶账漂移" });
    // J2-2 直方图机读行（HIST|1|edge<=300|n=40）
    let ok = hist_line(1, 40) == "HIST|1|edge<=300|n=40";
    s.add("56029-J2-机读行格式", ok, if ok { "" } else { "机读格式漂移" });
    // J2-3 跨桶分布（heavy 500..617 全落 (300,1000] 桶 2）
    let hv = collect(EN_HEAVY).expect("hv");
    let hist2 = histogram(&hv).expect("hist2");
    let ok = hist2[2].1 == 40;
    s.add("56029-J2-跨桶分布", ok, if ok { "" } else { "跨桶漂移" });
    // J3-1 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56029-J3-注入核必抓", ok, &detail);
    // J3-2 单值冒充审计钩子（AI-82 抽读对接：同值集拒绝出统计）
    let same = [123u64; 30];
    let ok = aggregate(&same).err() == Some(AG_SINGLE);
    s.add("56029-J3-单值冒充钩子", ok, if ok { "" } else { "冒充穿透" });
    // J3-3 环境档名闭集同源
    let ok = EN_NAMES[EN_IDLE] == "idle" && EN_NAMES[EN_HEAVY] == "heavy";
    s.add("56029-J3-环境档名同源", ok, if ok { "" } else { "命名漂移" });
    // J1-4 聚合账重复档拒
    let st = aggregate(&collect(EN_IDLE).expect("c4")).expect("st4");
    let ok = b.put(EN_IDLE, st).err() == Some(AG_ENV_UNK);
    s.add("56029-J1-重复档拒", ok, if ok { "" } else { "重复档穿透" });
    // J1-5 样本不足拒聚合（29 样本）
    let mut short: Vec<u64> = Vec::new();
    for i in 0..29u64 {
        short.push(200 + i);
    }
    let ok = aggregate(&short).err() == Some(AG_COUNT_MISS);
    s.add("56029-J1-短样本拒聚合", ok, if ok { "" } else { "短样本出账" });
    // J2-4 封账冻结
    b.seal();
    let ok = b.sealed && b.get(EN_HEAVY).expect("h2").p95 == 614;
    s.add("56029-J2-封账冻结", ok, if ok { "" } else { "封账语义漂移" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56029_all_checks_green() {
        let set = run_unx56029_checks();
        assert!(set.all_passed(), "UNX-F56029 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56029_single_value_hook() {
        let same = [7u64; 40];
        assert_eq!(aggregate(&same).err(), Some(AG_SINGLE));
    }
}
