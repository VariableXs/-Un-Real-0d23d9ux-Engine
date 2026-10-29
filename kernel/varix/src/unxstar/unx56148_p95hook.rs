//! UNX-F56148 ratio_p95 计算与审计钩子（0x61280 段）
//!
//! 锚点：UNX-F56148-J1 p95 计算账（ratio 样本集 p95 分位——最近邻秩
//! 法整数域——逐批落账：批键 × p95 值 × 样本量 × 指纹四元组）；J2 秩
//! 法语义（升序第 ⌈0.95×n⌉ 位，n=1 时取唯一值，边界批全验证）；J3 审
//! 计钩子（p95 批算毕钩子必触发，钩子账含批指纹留痕）三判据全绿。
//! 域：CoRun·UNX-F2-B40（账本域·p95 审计面）。
//!
//! 结构：ratio 样本批（升序插入 + 秩位取值）+ p95 核（最近邻秩整数域）
//! + 批账（批键唯一 + 批指纹折叠）+ 审计钩子（批算毕回调记账，钩子
//! 触发计数可查）+ 注入核（空批求 p95/秩位越界/重复批键/钩子漏触发/
//! 批内篡改必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x61280 独占 0x20）。
pub const SEG_BASE: u32 = 0x61280;
pub const P95_EMPTY_BATCH: u32 = SEG_BASE + 0x01; // 空批求 p95
pub const P95_RANK_RANGE: u32 = SEG_BASE + 0x02; // 秩位越界
pub const P95_DUP_BATCH: u32 = SEG_BASE + 0x03; // 重复批键
pub const P95_HOOK_MISS: u32 = SEG_BASE + 0x04; // 钩子漏触发
pub const P95_BATCH_TAMPER: u32 = SEG_BASE + 0x05; // 批内篡改

/// 最近邻秩法 p95：升序第 ⌈0.95×n⌉ 位（1 起），n=1 取唯一值。
/// 返回秩位（1 起）由调用方取升序数组对应元素；空批拒。
pub fn p95_rank(n: u32) -> Result<u32, u32> {
    if n == 0 {
        return Err(P95_EMPTY_BATCH);
    }
    // ⌈0.95×n⌉ = (19×n + 19) / 20 整数上取整（19/20 = 0.95）
    Ok((19 * n + 19) / 20)
}

/// p95 取值核：给定升序样本集直接返回 p95 值。
pub fn p95_of(sorted: &[u32]) -> Result<u32, u32> {
    let rank = p95_rank(sorted.len() as u32)? as usize;
    if rank == 0 || rank > sorted.len() {
        return Err(P95_RANK_RANGE);
    }
    Ok(sorted[rank - 1])
}

/// 升序插入核：保持升序的插入位置（二分）。
pub fn insert_pos(sorted: &[u32], v: u32) -> usize {
    let mut lo = 0usize;
    let mut hi = sorted.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if sorted[mid] <= v {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

/// 单条 p95 批账。
#[derive(Clone)]
pub struct P95Entry {
    pub batch_key: String,
    pub p95_val: u32,
    pub sample_n: u32,
    pub batch_fp: u32,
}

impl P95Entry {
    fn render_line(&self) -> String {
        format!(
            "{}|p95{}|n{}|{:08x}",
            self.batch_key, self.p95_val, self.sample_n, self.batch_fp
        )
    }
}

/// p95 批账 + 审计钩子。
pub struct P95Ledger {
    entries: Vec<P95Entry>,
    hook_fired: u32,
    hook_log: Vec<String>,
}

impl P95Ledger {
    pub fn new() -> Self {
        P95Ledger {
            entries: Vec::new(),
            hook_fired: 0,
            hook_log: Vec::new(),
        }
    }

    /// 提交一批：升序副本求 p95 → 落账 → 触发审计钩子。
    pub fn commit_batch(&mut self, batch_key: &str, mut samples: Vec<u32>) -> Result<u32, u32> {
        if self.entries.iter().any(|e| e.batch_key == batch_key) {
            return Err(P95_DUP_BATCH);
        }
        if samples.is_empty() {
            return Err(P95_EMPTY_BATCH);
        }
        samples.sort_unstable();
        let val = p95_of(&samples)?;
        let mut h: u32 = 0x811C_9DC5;
        for v in &samples {
            h = h.wrapping_mul(0x0100_0193) ^ *v;
        }
        let fp = h;
        self.entries.push(P95Entry {
            batch_key: batch_key.to_string(),
            p95_val: val,
            sample_n: samples.len() as u32,
            batch_fp: fp,
        });
        // 审计钩子：批算毕必触发
        self.hook_fired += 1;
        self.hook_log.push(format!("hook:{}|p95{}|n{}", batch_key, val, samples.len()));
        Ok(val)
    }

    pub fn entry(&self, batch_key: &str) -> Option<&P95Entry> {
        self.entries.iter().find(|e| e.batch_key == batch_key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn hook_count(&self) -> u32 {
        self.hook_fired
    }

    /// 钩子一致性：触发数 = 落账批数。
    pub fn hooks_consistent(&self) -> bool {
        self.hook_fired as usize == self.entries.len()
            && self.hook_log.len() == self.entries.len()
    }

    /// 批篡改核：重算批指纹与存量一致（防落账后被改）。
    /// 注：本核以批键重放固定样本序列验证（审计重放语义）。
    pub fn replay_ok(&self, batch_key: &str, samples: &[u32]) -> bool {
        match self.entry(batch_key) {
            None => false,
            Some(e) => {
                let mut h: u32 = 0x811C_9DC5;
                for v in samples {
                    h = h.wrapping_mul(0x0100_0193) ^ *v;
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
        for l in &self.hook_log {
            out.push_str(l);
            out.push('\n');
        }
        out
    }
}

impl Default for P95Ledger {
    fn default() -> Self {
        Self::new()
    }
}

/// 标准夹具：三批账（20 样本 / 1 样本 / 40 样本）。
fn standard_ledger() -> P95Ledger {
    let mut led = P95Ledger::new();
    // 批一：20 样本 1..20（升序）→ 秩位 ⌈19⌉=19 → 值 19
    let b1: Vec<u32> = (1..=20).collect();
    let _ = led.commit_batch("fps_full", b1);
    // 批二：1 样本 → 唯一值
    let _ = led.commit_batch("fps_single", alloc::vec![9750]);
    // 批三：40 样本偶数序列 → 秩位 38 → 值 76
    let b3: Vec<u32> = (1..=40).map(|x| x * 2).collect();
    let _ = led.commit_batch("fps_idle", b3);
    led
}

/// 注入核：五类非法操作必抓计数。
fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;
    // 注 1：空批
    let mut led = P95Ledger::new();
    if led.commit_batch("x", Vec::new()) == Err(P95_EMPTY_BATCH) {
        caught += 1;
    }
    // 注 2：重复批键
    let mut led = standard_ledger();
    if led.commit_batch("fps_full", alloc::vec![1, 2, 3]) == Err(P95_DUP_BATCH) {
        caught += 1;
    }
    // 注 3：秩位越界（p95_of 传空数组经 rank 拒）
    if p95_of(&[]) == Err(P95_EMPTY_BATCH) {
        caught += 1;
    }
    // 注 4：钩子漏触发（人工破坏钩子计数后一致性核必假）
    let mut led = standard_ledger();
    led.hook_fired = 0;
    if !led.hooks_consistent() {
        caught += 1;
    }
    // 注 5：批篡改（重放异样本指纹必不匹配）
    let led = standard_ledger();
    if !led.replay_ok("fps_full", &(1..=20).map(|x| x + 1).collect::<Vec<u32>>()) {
        caught += 1;
    }
    (caught, total)
}

/// 判据：J1 p95 账；J2 秩法语义；J3 审计钩子。
pub fn run_unx56148_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56148");
    // J1-1 三批落账
    let led = standard_ledger();
    let ok = led.len() == 3 && led.hooks_consistent();
    s.add("56148-J1-三批落账", ok, if ok { "" } else { "批账不全" });
    // J1-2 渲染导出齐
    let text = standard_ledger().render_ledger();
    let ok = text.contains("fps_full|p9519|n20|") && text.contains("hook:fps_idle|");
    s.add("56148-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // J2-1 秩法锚：n=20 → 秩 19
    let ok = p95_rank(20) == Ok(19);
    s.add("56148-J2-秩20锚", ok, if ok { "" } else { "秩法漂移" });
    // J2-2 秩法锚：n=1 → 秩 1
    let ok = p95_rank(1) == Ok(1);
    s.add("56148-J2-秩1锚", ok, if ok { "" } else { "单样本漂移" });
    // J2-3 秩法锚：n=40 → 秩 38
    let ok = p95_rank(40) == Ok(38);
    s.add("56148-J2-秩40锚", ok, if ok { "" } else { "秩法漂移" });
    // J2-4 p95 取值：1..20 秩 19 → 值 19
    let asc: alloc::vec::Vec<u32> = (1..=20).collect();
    let ok = p95_of(&asc) == Ok(19);
    s.add("56148-J2-p95取值", ok, if ok { "" } else { "取值漂移" });
    // J2-5 空批拒
    let ok = p95_rank(0) == Err(P95_EMPTY_BATCH);
    s.add("56148-J2-空批拒", ok, if ok { "" } else { "空批穿透" });
    // J2-6 升序插入核：中位插入位置
    let base = [10u32, 20, 30];
    let ok = insert_pos(&base, 15) == 1 && insert_pos(&base, 35) == 3 && insert_pos(&base, 5) == 0;
    s.add("56148-J2-插入位", ok, if ok { "" } else { "插入核漂移" });
    // J3-1 钩子触发数 = 批数
    let ok = standard_ledger().hook_count() == 3;
    s.add("56148-J3-钩子计数", ok, if ok { "" } else { "钩子漂移" });
    // J3-2 钩子日志留痕带批键
    let led = standard_ledger();
    let ok = led.hook_log.iter().any(|l| l.starts_with("hook:fps_full|"));
    s.add("56148-J3-钩子留痕", ok, if ok { "" } else { "日志面漂移" });
    // J3-3 重放验证过
    let b1: alloc::vec::Vec<u32> = (1..=20).collect();
    let ok = standard_ledger().replay_ok("fps_full", &b1);
    s.add("56148-J3-重放验证", ok, if ok { "" } else { "重放漂移" });
    // J3-4 单样本批 p95 = 唯一值
    let led = standard_ledger();
    let ok = led.entry("fps_single").map(|e| e.p95_val == 9750 && e.sample_n == 1).unwrap_or(false);
    s.add("56148-J3-单样本批", ok, if ok { "" } else { "单样本漂移" });
    // 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56148-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56148_all_checks_green() {
        let set = run_unx56148_checks();
        assert!(set.all_passed(), "UNX-F56148 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56148_rank_stable() {
        assert_eq!(p95_rank(20), Ok(19));
    }
}
