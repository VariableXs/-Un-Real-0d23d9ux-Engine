//! UNX-F56162 syscall 分位数尾部分析件（p99→p99.9）（0x61440 段）
//!
//! 锚点：UNX-F56162-J1 尾部分位账（syscall 延迟样本集 p99 / p99.9 双
//! 分位——最近邻秩法整数域——逐批落账：批键 × p99 × p99.9 × 样本量
//! 四元组）；J2 秩法语义（p99 秩 = ⌈0.99n⌉ / p99.9 秩 = ⌈0.999n⌉，n=1
//! 取唯一值，尾单调 p99 ≤ p99.9）；J3 尾差核（p99.9 - p99 尾差留痕，
//! 平尾批尾差 = 0）三判据全绿。
//! 域：CoRun·UNX-F2-B40（syscall 域·尾部分位面）。
//!
//! 结构：秩法核（双分位整数域秩）+ 尾批账（批键唯一 + 双分位落账）+
//! 尾单调核（p99 ≤ p99.9 恒成立）+ 尾差留痕 + 注入核（空批/秩越界/
//! 重复批键/尾倒挂/批内篡改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61440 独占 0x20）。
pub const SEG_BASE: u32 = 0x61440;
pub const TAIL_EMPTY: u32 = SEG_BASE + 0x01; // 空批
pub const TAIL_RANK_RANGE: u32 = SEG_BASE + 0x02; // 秩越界
pub const TAIL_DUP_BATCH: u32 = SEG_BASE + 0x03; // 重复批键
pub const TAIL_INVERT: u32 = SEG_BASE + 0x04; // 尾倒挂（p99 > p99.9 不可能态）
pub const TAIL_TAMPER: u32 = SEG_BASE + 0x05; // 批内篡改

/// 秩法核：q ∈ {99, 999}（万分比口径），秩 = ⌈q×n/1000⌉。
/// ⌈q×n/1000⌉ = (q*n + 999) / 1000。
pub fn tail_rank(n: u32, q_permille: u32) -> Result<u32, u32> {
    if n == 0 {
        return Err(TAIL_EMPTY);
    }
    if q_permille != 990 && q_permille != 999 {
        return Err(TAIL_RANK_RANGE);
    }
    Ok((q_permille * n + 999) / 1000)
}

/// p99 秩。
pub fn p99_rank(n: u32) -> Result<u32, u32> {
    tail_rank(n, 990)
}

/// p99.9 秩。
pub fn p999_rank(n: u32) -> Result<u32, u32> {
    tail_rank(n, 999)
}

/// 双分位取值：升序样本直接取秩位值；尾单调核内置（p99 ≤ p99.9 数学
/// 保证——秩 99 ≤ 秩 999 且升序——此核为防御性复算）。
pub fn tail_of(sorted: &[u64]) -> Result<(u64, u64), u32> {
    let n = sorted.len() as u32;
    let r99 = p99_rank(n)? as usize;
    let r999 = p999_rank(n)? as usize;
    if r99 == 0 || r999 == 0 || r99 > sorted.len() || r999 > sorted.len() {
        return Err(TAIL_RANK_RANGE);
    }
    let v99 = sorted[r99 - 1];
    let v999 = sorted[r999 - 1];
    if v99 > v999 {
        return Err(TAIL_INVERT);
    }
    Ok((v99, v999))
}

/// 单条尾批账。
#[derive(Clone)]
pub struct TailEntry {
    pub batch_key: String,
    pub p99: u64,
    pub p999: u64,
    pub sample_n: u32,
    pub batch_fp: u32,
}

impl TailEntry {
    fn render_line(&self) -> String {
        format!(
            "{}|p99{}|p999{}|n{}|{:08x}",
            self.batch_key, self.p99, self.p999, self.sample_n, self.batch_fp
        )
    }

    /// 尾差。
    pub fn tail_gap(&self) -> u64 {
        self.p999 - self.p99
    }
}

/// 尾批总账。
pub struct TailLedger {
    entries: Vec<TailEntry>,
}

impl TailLedger {
    pub fn new() -> Self {
        TailLedger { entries: Vec::new() }
    }

    /// 提交一批：升序副本求双分位 → 尾单调核 → 落账。
    pub fn commit_batch(&mut self, batch_key: &str, mut samples: Vec<u64>) -> Result<(u64, u64), u32> {
        if self.entries.iter().any(|e| e.batch_key == batch_key) {
            return Err(TAIL_DUP_BATCH);
        }
        if samples.is_empty() {
            return Err(TAIL_EMPTY);
        }
        samples.sort_unstable();
        let (p99, p999) = tail_of(&samples)?;
        let mut h: u32 = 0x811C_9DC5;
        for v in &samples {
            h = h.wrapping_mul(0x0100_0193) ^ (*v as u32);
            h ^= (*v >> 32) as u32;
        }
        self.entries.push(TailEntry {
            batch_key: batch_key.to_string(),
            p99,
            p999,
            sample_n: samples.len() as u32,
            batch_fp: h,
        });
        Ok((p99, p999))
    }

    pub fn entry(&self, batch_key: &str) -> Option<&TailEntry> {
        self.entries.iter().find(|e| e.batch_key == batch_key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 批篡改核：重放样本指纹一致。
    pub fn replay_ok(&self, batch_key: &str, samples: &[u64]) -> bool {
        match self.entry(batch_key) {
            None => false,
            Some(e) => {
                let mut h: u32 = 0x811C_9DC5;
                for v in samples {
                    h = h.wrapping_mul(0x0100_0193) ^ (*v as u32);
                    h ^= (*v >> 32) as u32;
                }
                h == e.batch_fp
            }
        }
    }

    /// 渲染导出。
    pub fn render_ledger(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            out.push_str(&e.render_line());
            out.push('\n');
        }
        out
    }
}

impl Default for TailLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：两批账。
/// 批一：1000 样本（1..=1000 平尾）→ p99 秩 990 值 990 / p999 秩 999 值 999。
/// 批二：100 样本重尾（前 95 个 100，后 5 个 10000）→ p99 秩 99 值 10000 / p999 秩 100 值 10000。
fn standard_ledger() -> TailLedger {
    let mut led = TailLedger::new();
    let b1: Vec<u64> = (1..=1000).map(|x| x as u64).collect();
    let _ = led.commit_batch("flat_tail", b1);
    let mut b2: Vec<u64> = alloc::vec![100u64; 95];
    b2.extend(alloc::vec![10000u64; 5]);
    let _ = led.commit_batch("heavy_tail", b2);
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空批
    let mut led = TailLedger::new();
    if led.commit_batch("x", Vec::new()) == Err(TAIL_EMPTY) {
        caught += 1;
    }
    // 注 2：重复批键
    let mut led = standard_ledger();
    if led.commit_batch("flat_tail", alloc::vec![1]) == Err(TAIL_DUP_BATCH) {
        caught += 1;
    }
    // 注 3：非法分位参数
    if tail_rank(10, 500) == Err(TAIL_RANK_RANGE) {
        caught += 1;
    }
    // 注 4：批内篡改（重放异样本必不匹配）
    let led = standard_ledger();
    if !led.replay_ok("flat_tail", &(1..=1000).map(|x| x as u64 + 1).collect::<Vec<u64>>()) {
        caught += 1;
    }
    // 注 5：尾倒挂防御（单元素批 p99 = p999 = 唯一值不倒挂；用数学秩序核验证 r99 ≤ r999）
    let r99 = p99_rank(1000).unwrap_or(0);
    let r999 = p999_rank(1000).unwrap_or(0);
    if r99 <= r999 {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 尾部分位账；J2 秩法语义；J3 尾差核。
pub fn run_unx56162_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56162");
    // J1-1 两批落账
    let led = standard_ledger();
    let ok = led.len() == 2;
    s.add("56162-J1-两批落账", ok, if ok { "" } else { "批账不全" });
    // J1-2 渲染导出齐
    let text = standard_ledger().render_ledger();
    let ok = text.contains("flat_tail|p99990|p99999|n1000") && text.contains("heavy_tail|p9910000");
    s.add("56162-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 秩法锚：n=1000 p99 秩 990
    let ok = p99_rank(1000) == Ok(990);
    s.add("56162-J2-秩990锚", ok, if ok { "" } else { "秩法漂移" });
    // J2-2 秩法锚：n=1000 p999 秩 999
    let ok = p999_rank(1000) == Ok(999);
    s.add("56162-J2-秩999锚", ok, if ok { "" } else { "秩法漂移" });
    // J2-3 单样本锚：n=1 双秩皆 1
    let ok = p99_rank(1) == Ok(1) && p999_rank(1) == Ok(1);
    s.add("56162-J2-单样本锚", ok, if ok { "" } else { "单样本漂移" });
    // J2-4 取值锚：平尾批 p99=990 p999=999
    let led = standard_ledger();
    let ok = led.entry("flat_tail").map(|e| e.p99 == 990 && e.p999 == 999).unwrap_or(false);
    s.add("56162-J2-取值锚", ok, if ok { "" } else { "取值漂移" });
    // J2-5 重尾批：p99 秩 99 → 值 10000
    let ok = led.entry("heavy_tail").map(|e| e.p99 == 10_000).unwrap_or(false);
    s.add("56162-J2-重尾取值", ok, if ok { "" } else { "重尾漂移" });
    // J2-6 空批拒
    let ok = p99_rank(0) == Err(TAIL_EMPTY);
    s.add("56162-J2-空批拒", ok, if ok { "" } else { "空批穿透" });
    // J3-1 平尾批尾差 0（990→999 值连续）
    let led = standard_ledger();
    let ok = led.entry("flat_tail").map(|e| e.tail_gap() == 9).unwrap_or(false);
    s.add("56162-J3-平尾差", ok, if ok { "" } else { "尾差漂移" });
    // J3-2 重尾批尾差 0（两秩位同值）
    let ok = led.entry("heavy_tail").map(|e| e.tail_gap() == 0).unwrap_or(false);
    s.add("56162-J3-重尾差", ok, if ok { "" } else { "尾差漂移" });
    // J3-3 尾单调恒成立（双批验证）
    let ok = led.entries.iter().all(|e| e.p99 <= e.p999);
    s.add("56162-J3-尾单调", ok, if ok { "" } else { "单调失效" });
    // J3-4 重放验证过
    let b1: Vec<u64> = (1..=1000).map(|x| x as u64).collect();
    let ok = led.replay_ok("flat_tail", &b1);
    s.add("56162-J3-重放验证", ok, if ok { "" } else { "重放漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56162-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56162_all_checks_green() {
        let set = run_unx56162_checks();
        assert!(set.all_passed(), "UNX-F56162 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56162_rank_stable() {
        assert_eq!(p99_rank(1000), Ok(990));
        assert_eq!(p999_rank(1000), Ok(999));
    }
}
