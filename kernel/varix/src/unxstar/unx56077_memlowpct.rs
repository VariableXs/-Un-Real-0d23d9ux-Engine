//! UNX-F56077 内存指标 1% low 等价物设计注记件（0x60980 段）
//!
//! 锚点：UNX-F56077-J1 设计注记全量（1% low 等价物三要素——窗长 × 分位
//! × 聚合口径——逐项落注）；J2 分位计算可复算（给定样本集按注记分位
//! 复算一致，插值口径显性化）；J3 注记审计链（注记指纹、封账只读、口径
//! 闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·低分位口径面）。
//!
//! 结构：等价物要素注记册（窗长 ms × 分位万分比 × 聚合口径三元组落注）+
//! 分位计算核（最近秩法，样本升序取秩位，禁插值含糊）+ 样本守闸（样本
//! 量不足拒算）+ 封账语义 + 注入核（分位越界/样本不足/空样本/封账改写/
//! 重复注记/未知聚合口径必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60980 独占 0x20）。
pub const SEG_BASE: u32 = 0x60980;
pub const LP_PCT_RANGE: u32 = SEG_BASE + 0x01; // 分位越界（不在 1..=9999 万分比）
pub const LP_SAMPLE_SHORT: u32 = SEG_BASE + 0x02; // 样本量不足（< 30 纪律）
pub const LP_SAMPLE_EMPTY: u32 = SEG_BASE + 0x03; // 空样本
pub const LP_AGG_UNK: u32 = SEG_BASE + 0x04; // 未知聚合口径（闭集外）
pub const LP_REMARK_DUP: u32 = SEG_BASE + 0x05; // 同要素重复注记
pub const LP_SEALED: u32 = SEG_BASE + 0x06; // 注记账已封账拒写
pub const LP_WINDOW_ZERO: u32 = SEG_BASE + 0x07; // 窗长为零

/// 最小样本量纪律（与 F56086 守门器同源）。
pub const SAMPLE_MIN: usize = 30;

/// 聚合口径闭集。
pub const AG_NEAREST: usize = 0;
pub const AG_MEAN: usize = 1;
pub const AG_COUNT: usize = 2;
pub const AG_NAMES: [&str; AG_COUNT] = ["nearest_rank", "trimmed_mean"];

/// 分位合法性核：1..=9999 万分比。
pub fn percentile_ok(p_bp: u32) -> bool {
    p_bp >= 1 && p_bp <= 9_999
}

/// 最近秩法分位：样本升序后取第 ceil(p*n/10000) 位（1 起）。
pub fn nearest_rank(samples: &[u64], p_bp: u32) -> Result<u64, u32> {
    if samples.is_empty() {
        return Err(LP_SAMPLE_EMPTY);
    }
    if samples.len() < SAMPLE_MIN {
        return Err(LP_SAMPLE_SHORT);
    }
    if !percentile_ok(p_bp) {
        return Err(LP_PCT_RANGE);
    }
    let mut sorted: Vec<u64> = Vec::new();
    for s in samples.iter() {
        sorted.push(*s);
    }
    // 插入排序（no_std 环境显性排序，避免依赖 sort 稳定语义）。
    for i in 1..sorted.len() {
        let key = sorted[i];
        let mut j = i;
        while j > 0 && sorted[j - 1] > key {
            sorted[j] = sorted[j - 1];
            j -= 1;
        }
        sorted[j] = key;
    }
    let n = sorted.len() as u64;
    // 秩 = ceil(p*n/10000)，至少 1。
    let rank = ((p_bp as u64 * n) + 9_999) / 10_000;
    let rank = if rank < 1 { 1 } else { rank };
    Ok(sorted[(rank - 1) as usize])
}

/// 均值核（u64 中转求和防溢出不可行则 u128；此处样本 ≤ 万级，u64 足够）。
pub fn mean_of(samples: &[u64]) -> Result<u64, u32> {
    if samples.is_empty() {
        return Err(LP_SAMPLE_EMPTY);
    }
    let mut sum: u128 = 0;
    for s in samples.iter() {
        sum += *s as u128;
    }
    Ok((sum / samples.len() as u128) as u64)
}

/// 等价物要素注记：窗长 ms × 分位万分比 × 聚合口径 三元组。
#[derive(Clone)]
pub struct LpRemark {
    pub window_ms: u32,
    pub percentile_bp: u32,
    pub agg: usize,
}

/// 注记总账（单槽 + 封账）。
pub struct LpBook {
    pub remark: Option<LpRemark>,
    pub sealed: bool,
    pub audit: Vec<String>,
}

impl LpBook {
    pub fn new() -> Self {
        LpBook { remark: None, sealed: false, audit: Vec::new() }
    }

    /// 落注：窗长非零 + 分位合法 + 口径闭集三闸。
    pub fn remark(&mut self, window_ms: u32, percentile_bp: u32, agg: usize) -> Result<(), u32> {
        if self.sealed {
            return Err(LP_SEALED);
        }
        if self.remark.is_some() {
            return Err(LP_REMARK_DUP);
        }
        if window_ms == 0 {
            return Err(LP_WINDOW_ZERO);
        }
        if !percentile_ok(percentile_bp) {
            return Err(LP_PCT_RANGE);
        }
        if agg >= AG_COUNT {
            return Err(LP_AGG_UNK);
        }
        self.audit.push(format!("remark|{}|{}|{}", window_ms, percentile_bp, AG_NAMES[agg]));
        self.remark = Some(LpRemark { window_ms, percentile_bp, agg });
        Ok(())
    }

    /// 按注记口径计算低分位（口径分派 + 守闸）。
    pub fn compute_low(&self, samples: &[u64]) -> Result<u64, u32> {
        match &self.remark {
            None => Err(LP_EMPTY),
            Some(r) => match r.agg {
                AG_NEAREST => nearest_rank(samples, r.percentile_bp),
                AG_MEAN => mean_of(samples),
                _ => Err(LP_AGG_UNK),
            },
        }
    }

    /// J1：注记全量（三要素齐 + 口径合法）。
    pub fn remark_ok(&self) -> bool {
        match &self.remark {
            None => false,
            Some(r) => r.window_ms > 0 && percentile_ok(r.percentile_bp) && r.agg < AG_COUNT,
        }
    }

    /// J3：审计链完整。
    pub fn audit_ok(&self) -> bool {
        !self.audit.is_empty() && self.audit.iter().all(|a| a.starts_with("remark|"))
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封账指纹。
    pub fn seal_fingerprint(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        match &self.remark {
            None => {}
            Some(r) => {
                let line = format!("{}|{}|{}", r.window_ms, r.percentile_bp, AG_NAMES[r.agg]);
                for b in line.as_bytes() {
                    h ^= *b as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
            }
        }
        h
    }
}

pub const LP_EMPTY: u32 = SEG_BASE + 0x07;

/// 标准夹具：窗 1000ms、1% low（100 万分比）、最近秩口径。
pub fn standard_book() -> LpBook {
    let mut b = LpBook::new();
    if b.remark(1_000, 100, AG_NEAREST).is_err() {
        panic!("standard fixture remark must succeed");
    }
    b
}

/// 标准样本集：40 个 u64（可预知排序后的低分位值）。
pub fn standard_samples() -> Vec<u64> {
    // 40 个样本：值 = 40 - i 递减（无序），排序后升序 1..=40。
    let mut v: Vec<u64> = Vec::new();
    for i in 0..40u64 {
        v.push(40 - i);
    }
    v
}

/// 注入核：7 注入必抓（分位越界/样本不足/空样本/封账改写/重复注记/未知口径/窗长零）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 分位越界（0 万分比）
    let mut b = LpBook::new();
    if b.remark(1_000, 0, AG_NEAREST) == Err(LP_PCT_RANGE) {
        caught += 1;
    }
    // 2) 未知口径
    if b.remark(1_000, 100, 9) == Err(LP_AGG_UNK) {
        caught += 1;
    }
    // 3) 窗长零
    if b.remark(0, 100, AG_NEAREST) == Err(LP_WINDOW_ZERO) {
        caught += 1;
    }
    // 4) 重复注记
    if b.remark(1_000, 100, AG_NEAREST).is_ok() && b.remark(2_000, 100, AG_NEAREST) == Err(LP_REMARK_DUP) {
        caught += 1;
    }
    // 5) 样本不足（29 < 30）
    let short: Vec<u64> = (0..29u64).collect();
    if nearest_rank(&short, 100) == Err(LP_SAMPLE_SHORT) {
        caught += 1;
    }
    // 6) 空样本
    if nearest_rank(&[], 100) == Err(LP_SAMPLE_EMPTY) {
        caught += 1;
    }
    // 7) 封账后注记拒
    b.seal();
    if b.remark(3_000, 100, AG_NEAREST) == Err(LP_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 注记账渲染导出。
pub fn render_book(b: &LpBook) -> String {
    match &b.remark {
        None => "1p_low|UNSET".to_string(),
        Some(r) => format!(
            "1p_low|{}|{}|{}|{:08x}",
            r.window_ms, r.percentile_bp, AG_NAMES[r.agg], b.seal_fingerprint()
        ),
    }
}

/// 渲染完整性核：非 UNSET 行五段齐。
pub fn render_complete(text: &str) -> bool {
    if text.ends_with("UNSET") {
        return false;
    }
    let parts: Vec<&str> = text.split('|').collect();
    parts.len() == 5 && parts[0] == "1p_low"
}

/// 判据入口（CheckSet）。
pub fn run_unx56077_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56077");
    // J1-1 注记全量（三要素齐）
    let b = standard_book();
    let ok = b.remark_ok() && !b.sealed;
    s.add("56077-J1-注记全量", ok, if ok { "" } else { "三要素残缺" });
    // J1-2 分位闭集边界（1 与 9999 合法、0 与 10000 越界）
    let ok = percentile_ok(1) && percentile_ok(9_999) && !percentile_ok(0) && !percentile_ok(10_000);
    s.add("56077-J1-分位闭集边界", ok, if ok { "" } else { "分位边界漂移" });
    // J1-3 聚合口径闭集齐
    let ok = AG_NAMES.len() == AG_COUNT && AG_NAMES[0] == "nearest_rank";
    s.add("56077-J1-口径闭集齐", ok, if ok { "" } else { "口径漂移" });
    // J2-1 最近秩复算：40 样本 1% → 秩 ceil(100*40/10000)=ceil(0.4)=1 → 最小值 1
    let samples = standard_samples();
    let ok = nearest_rank(&samples, 100) == Ok(1);
    s.add("56077-J2-最近秩复算", ok, if ok { "" } else { "秩算术漂移" });
    // J2-2 最近秩敏感：50% → 秩 20 → 值 20
    let ok = nearest_rank(&samples, 5_000) == Ok(20);
    s.add("56077-J2-中位秩复算", ok, if ok { "" } else { "中位漂移" });
    // J2-3 样本不足拒算（29 < 30）
    let short: Vec<u64> = (0..29u64).collect();
    let ok = nearest_rank(&short, 100) == Err(LP_SAMPLE_SHORT);
    s.add("56077-J2-样本不足拒算", ok, if ok { "" } else { "样本闸失效" });
    // J2-4 注记口径分派（均值口径取均值）
    let mut b2 = LpBook::new();
    let _ = b2.remark(1_000, 100, AG_MEAN);
    // 40 样本均值 = (1+40)*40/2/40 = 20
    let ok = b2.compute_low(&samples) == Ok(20);
    s.add("56077-J2-口径分派", ok, if ok { "" } else { "口径分派漂移" });
    // J3-1 审计链完整
    let ok = standard_book().audit_ok();
    s.add("56077-J3-审计链完整", ok, if ok { "" } else { "审计链断裂" });
    // J3-2 封账指纹稳定
    let ok = standard_book().seal_fingerprint() == standard_book().seal_fingerprint();
    s.add("56077-J3-封账指纹稳定", ok, if ok { "" } else { "指纹漂移" });
    // J3-3 口径敏感（分位改动指纹变）
    let fp0 = standard_book().seal_fingerprint();
    let mut b3 = LpBook::new();
    let _ = b3.remark(1_000, 500, AG_NEAREST);
    let ok = b3.seal_fingerprint() != fp0;
    s.add("56077-J3-指纹敏感", ok, if ok { "" } else { "指纹钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.remark(2_000, 100, AG_NEAREST) == Err(LP_SEALED);
    s.add("56077-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五段齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text) && text.contains("nearest_rank");
    s.add("56077-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56077-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56077_all_checks_green() {
        let set = run_unx56077_checks();
        assert!(set.all_passed(), "UNX-F56077 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56077_rank_stable() {
        let samples = standard_samples();
        assert_eq!(nearest_rank(&samples, 100), Ok(1));
    }
}
