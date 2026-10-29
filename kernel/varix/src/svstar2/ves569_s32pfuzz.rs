//! VE-F4335 · 一致性性能 fuzz（U07 性能与预算组）
//! 职责：性能面 fuzz——fuzz 目标三类（预算计算畸形输入 / 降级梯度乱序
//! 触发 / 卸载协议畸形切片）+ 性能退化 fuzz（构造病态输入使性能塌方——
//! 发现即立项优化）+ 发现即立案（最小化 + 优化排程）+ 误报白名单 + 配额
//! 限额调度 + 报告脱敏。
//! 段位：0x36E0（续表 idx569）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x36E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 性能塌方（病态输入发现——立项优化）。
    Collapse = SEG_BASE,
    /// 畸形输入发现。
    Malformed = SEG_BASE + 0x01,
    /// 乱序触发发现。
    OutOfOrder = SEG_BASE + 0x02,
    /// 配额耗尽。
    QuotaOut = SEG_BASE + 0x03,
    /// 立案钩子缺失。
    HookMissing = SEG_BASE + 0x04,
    /// 配额账漂移（分类计数与 used 不一致）。
    LedgerDrift = SEG_BASE + 0x05,
}

/// fuzz 目标三类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfTarget {
    BudgetCalc,
    DegradSeq,
    OffloadSlice,
}

/// 语料条目。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corpus {
    pub target: PfTarget,
    pub payload: u32,
    /// 病态输入标记（性能退化 fuzz 专用）。
    pub pathological: bool,
    pub sanitized: bool,
}

/// 立案单。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Case {
    pub seq: u32,
    pub code: PfCode,
    /// 塌方倍数（相对基线的劣化倍率 ×100）。
    pub collapse_x100: u32,
    pub minimized_len: u32,
}

/// 性能 fuzz 台。
#[derive(Default)]
pub struct PerfFuzz {
    pub corpus: Vec<Corpus>,
    pub cases: Vec<Case>,
    pub whitelist: Vec<u32>,
    pub quota: u32,
    pub used: u32,
    pub hook: bool,
    pub scheduled: u32,
    seq: u32,
    /// 深化：配额消耗分类账（静默/立案/拒绝三类计数）。
    pub used_silent: u32,
    pub used_case: u32,
    pub used_reject: u32,
}

impl PerfFuzz {
    pub fn new(quota: u32) -> Self {
        PerfFuzz { quota: quota.max(1), hook: true, ..Default::default() }
    }

    /// 语料入池（脱敏强制——本地池全脱敏态）。
    pub fn add_corpus(&mut self, target: PfTarget, payload: u32, pathological: bool) {
        self.corpus.push(Corpus { target, payload, pathological, sanitized: true });
    }

    /// 塌方倍数计算：病态输入耗时 / 基线耗时（×100）。
    pub fn collapse_x100(&self, base_ms: u32, patho_ms: u32) -> u32 {
        if base_ms == 0 {
            return 0;
        }
        patho_ms * 100 / base_ms
    }

    /// 发现处置：配额 → 白名单 → 钩子 → 立案（塌方立项优化排程计数）。
    pub fn discover(&mut self, payload: u32, code: PfCode, collapse_x100: u32) -> Result<Option<u32>, PfCode> {
        if self.used >= self.quota {
            self.used_reject += 1;
            return Err(PfCode::QuotaOut);
        }
        self.used += 1;
        if self.whitelist.contains(&payload) {
            self.used_silent += 1;
            return Ok(None);
        }
        if !self.hook {
            return Err(PfCode::HookMissing);
        }
        self.seq += 1;
        let min_len = minimal_len(payload);
        self.cases.push(Case { seq: self.seq, code, collapse_x100, minimized_len: min_len });
        if code == PfCode::Collapse {
            self.scheduled += 1; // 立项优化排程
        }
        self.used_case += 1;
        Ok(Some(self.seq))
    }

    /// 白名单登记（幂等）。
    pub fn whitelist_add(&mut self, payload: u32) {
        if !self.whitelist.contains(&payload) {
            self.whitelist.push(payload);
        }
    }

    /// 全案最小化义务核验。
    pub fn all_minimized(&self) -> bool {
        self.cases.iter().all(|c| c.minimized_len > 0 && c.minimized_len <= 16)
    }

    /// 塌方案均倍数（立项依据）。
    pub fn avg_collapse(&self) -> u32 {
        let cs: Vec<u32> = self.cases.iter().filter(|c| c.code == PfCode::Collapse).map(|c| c.collapse_x100).collect();
        if cs.is_empty() {
            return 0;
        }
        let sum: u64 = cs.iter().map(|v| *v as u64).sum();
        (sum / cs.len() as u64) as u32
    }

    /// 报告脱敏终检。
    pub fn export_clean(&self) -> bool {
        self.corpus.iter().all(|c| c.sanitized)
    }

    /// 拆钩（测试用）。
    pub fn detach_hook(&mut self) {
        self.hook = false;
    }

    // ─── 深化块：配额分类守恒账 ───

    /// 守恒：used ≡ 静默 + 立案（拒绝另计，不入 used）。
    /// 篡改任一计数即检出。
    pub fn quota_conserves(&self) -> bool {
        self.used == self.used_silent + self.used_case
            && self.used <= self.quota
    }

    /// 全分类账守恒（含拒绝）：静默+立案+拒绝 ≡ 总尝试。
    pub fn classify_conserves(&self, attempts: u32) -> bool {
        self.used_silent + self.used_case + self.used_reject == attempts
    }

    // ─── 深化块：塌方分位（整数 P50/P95）───

    /// 塌方案分位：p∈[0,100]，排序取 ceil(p/100*(n-1)) 位。
    pub fn collapse_percentile(&self, p: u32) -> u32 {
        let mut v: Vec<u32> = self.cases.iter()
            .filter(|c| c.code == PfCode::Collapse)
            .map(|c| c.collapse_x100)
            .collect();
        if v.is_empty() {
            return 0;
        }
        v.sort_unstable();
        let idx = ((p as u64 * (v.len() as u64 - 1) + 99) / 100) as usize;
        v[idx.min(v.len() - 1)]
    }

    // ─── 深化块：病态语料命中率 ───

    /// 命中率 bp：病态语料数 / 总语料数（整数 bp；空池恒零）。
    pub fn patho_hit_bp(&self) -> u32 {
        if self.corpus.is_empty() {
            return 0;
        }
        let n = self.corpus.iter().filter(|c| c.pathological).count() as u64;
        (n * 10000 / self.corpus.len() as u64) as u32
    }

    // ─── 深化块：塌方阈值筛查 ───

    /// 超 X 倍阈值塌方案序号表（立项优先级依据）。
    pub fn severe_cases(&self, x100_threshold: u32) -> Vec<u32> {
        self.cases.iter()
            .filter(|c| c.code == PfCode::Collapse && c.collapse_x100 >= x100_threshold)
            .map(|c| c.seq)
            .collect()
    }
}

fn minimal_len(payload: u32) -> u32 {
    if payload == 0 {
        return 1;
    }
    let mut v = payload;
    let mut n = 0u32;
    while v > 0 && n < 16 {
        v >>= 4;
        n += 1;
    }
    n
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves569_checks() -> CheckSet {
    let mut set = CheckSet::new("ves569");
    let mut fz = PerfFuzz::new(16);

    // C569-01 三类目标语料入池（含病态输入）。
    fz.add_corpus(PfTarget::BudgetCalc, 0xAB00, true);
    fz.add_corpus(PfTarget::DegradSeq, 0xCD00, false);
    fz.add_corpus(PfTarget::OffloadSlice, 0xEF00, true);
    chk(&mut set, "C569-01", fz.corpus.len() == 3 && fz.export_clean(), "三类语料脱敏入池");

    // C569-02 塌方倍数核算（基线 10ms 病态 2500ms = 250 倍）。
    chk(&mut set, "C569-02", fz.collapse_x100(10, 2500) == 25000, "塌方 250 倍");

    // C569-03 病态输入性能塌方发现 + 立项优化排程。
    let s1 = fz.discover(0xAB00, PfCode::Collapse, 25000);
    chk(&mut set, "C569-03", s1 == Ok(Some(1)) && fz.scheduled == 1, "塌方立项排程");

    // C569-04 畸形输入发现立案（无塌方）。
    let s2 = fz.discover(0xCD00, PfCode::Malformed, 0);
    chk(&mut set, "C569-04", s2 == Ok(Some(2)) && fz.scheduled == 1, "畸形立案不排程");

    // C569-05 乱序触发发现立案。
    let s3 = fz.discover(0xEF00, PfCode::OutOfOrder, 0);
    chk(&mut set, "C569-05", s3 == Ok(Some(3)), "乱序立案");

    // C569-06 全案最小化义务。
    chk(&mut set, "C569-06", fz.all_minimized(), "全案最小化");

    // C569-07 误报白名单静默跳过（幂等）。
    fz.whitelist_add(0xAB00);
    let s4 = fz.discover(0xAB00, PfCode::Collapse, 25000);
    fz.whitelist_add(0xAB00);
    chk(&mut set, "C569-07", s4 == Ok(None) && fz.whitelist.len() == 1, "白名单静默幂等");

    // C569-08 配额耗尽拒绝。
    let mut fz2 = PerfFuzz::new(2);
    let _ = fz2.discover(1, PfCode::Malformed, 0);
    let _ = fz2.discover(2, PfCode::OutOfOrder, 0);
    chk(&mut set, "C569-08", fz2.discover(3, PfCode::Malformed, 0) == Err(PfCode::QuotaOut), "配额耗尽拒");

    // C569-09 立案钩子缺失发现即拒。
    fz2.detach_hook();
    fz2.used = 0;
    chk(&mut set, "C569-09", fz2.discover(9, PfCode::Malformed, 0) == Err(PfCode::HookMissing), "缺钩发现即拒");

    // C569-10 塌方案均倍数。
    chk(&mut set, "C569-10", fz.avg_collapse() == 25000, "塌方案均");

    // C569-11 零基线塌方倍数防护（除零安全）。
    chk(&mut set, "C569-11", fz.collapse_x100(0, 100) == 0, "零基线防护");

    // C569-12 序号单调 + 配额下限钳制。
    let fz3 = PerfFuzz::new(0);
    let mono = fz.cases.windows(2).all(|w| w[0].seq < w[1].seq);
    chk(&mut set, "C569-12", fz3.quota == 1 && mono, "钳制+单调");

    // ── 深化判据：配额分类守恒账 ──
    // fz 已发生：3 立案 + 1 白名单静默 = used 4。
    chk(&mut set, "C569-13", fz.used == 4 && fz.used_case == 3
        && fz.used_silent == 1 && fz.quota_conserves(), "used≡静默+立案守恒");
    // 拒绝另计：fz2 拒 1 次。
    chk(&mut set, "C569-14", fz2.used_reject == 1 && fz2.classify_conserves(4),
        "拒绝另计全分类守恒");
    // 篡改检出。
    let mut fzt = PerfFuzz::new(8);
    let _ = fzt.discover(1, PfCode::Malformed, 0);
    fzt.used_case = 9;
    chk(&mut set, "C569-15", !fzt.quota_conserves(), "分类篡改守恒即破");

    // ── 深化判据：塌方分位 ──
    let mut fzp = PerfFuzz::new(8);
    let _ = fzp.discover(1, PfCode::Collapse, 100);
    let _ = fzp.discover(2, PfCode::Collapse, 500);
    let _ = fzp.discover(3, PfCode::Collapse, 900);
    let _ = fzp.discover(4, PfCode::Malformed, 0); // 非塌方不入分位
    chk(&mut set, "C569-16", fzp.collapse_percentile(0) == 100
        && fzp.collapse_percentile(50) == 500
        && fzp.collapse_percentile(100) == 900, "P0/P50/P100 三点分位");
    chk(&mut set, "C569-17", fzp.collapse_percentile(95) >= 500, "P95 落尾区");

    // ── 深化判据：病态命中率 + 重症筛查 ──
    chk(&mut set, "C569-18", fz.patho_hit_bp() == 6666, "2/3 病态命中 66.66%");
    chk(&mut set, "C569-19", fzp.severe_cases(500) == [2u32, 3u32]
        && fzp.severe_cases(1000).is_empty(), "重症筛查阈值线");

    // ── 深化判据：LCG 病态注入（发现必立案 + 白名单零误杀）──
    let mut rng: u64 = 0x5690_FACE;
    let mut fzi = PerfFuzz::new(64);
    let mut found = 0u32;
    for i in 0..64u64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let payload = (rng % 0xFFFF) as u32;
        let pathological = i % 8 == 0;
        fzi.add_corpus(PfTarget::BudgetCalc, payload, pathological);
        if pathological {
            found += 1;
            if fzi.discover(payload, PfCode::Collapse, 100 + (rng % 50) as u32).is_err() {
                found -= 1; // 发现必立案——失败即计数回退
            }
        }
    }
    chk(&mut set, "C569-20", found == 8 && fzi.cases.len() == 8
        && fzi.quota_conserves(), "64 轮注入 8 病态全立案");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_scheduling() {
        let mut fz = PerfFuzz::new(4);
        assert_eq!(fz.discover(7, PfCode::Collapse, 900), Ok(Some(1)));
        assert_eq!(fz.scheduled, 1);
        assert_eq!(fz.discover(8, PfCode::Malformed, 0), Ok(Some(2)));
        assert_eq!(fz.scheduled, 1); // 非塌方不排程
    }

    #[test]
    fn whitelist_and_quota() {
        let mut fz = PerfFuzz::new(1);
        fz.whitelist_add(5);
        assert_eq!(fz.discover(5, PfCode::Malformed, 0), Ok(None)); // 不耗配额外立案但耗配额？
        // 白名单命中发生在配额扣减后——used=1
        assert_eq!(fz.used, 1);
        assert_eq!(fz.discover(6, PfCode::Malformed, 0), Err(PfCode::QuotaOut));
    }

    #[test]
    fn hook_required() {
        let mut fz = PerfFuzz::new(4);
        fz.detach_hook();
        assert_eq!(fz.discover(1, PfCode::Malformed, 0), Err(PfCode::HookMissing));
    }

    #[test]
    fn classify_ledger_conserves() {
        let mut fz = PerfFuzz::new(4);
        fz.whitelist_add(7);
        let _ = fz.discover(7, PfCode::Malformed, 0); // 静默
        let _ = fz.discover(8, PfCode::Collapse, 900); // 立案
        let _ = fz.discover(9, PfCode::Malformed, 0); // 立案
        assert!(fz.quota_conserves());
        assert_eq!(fz.classify_conserves(3), true);
        let _ = fz.discover(10, PfCode::Malformed, 0); // 立案
        let _ = fz.discover(11, PfCode::Malformed, 0); // 拒绝（配额尽）
        assert!(fz.classify_conserves(5));
    }

    #[test]
    fn percentile_extremes() {
        let mut fz = PerfFuzz::new(4);
        assert_eq!(fz.collapse_percentile(50), 0); // 空案恒零
        let _ = fz.discover(1, PfCode::Collapse, 1000);
        assert_eq!(fz.collapse_percentile(0), 1000);
        assert_eq!(fz.collapse_percentile(100), 1000);
    }
}
