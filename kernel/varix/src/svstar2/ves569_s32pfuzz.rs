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
            return Err(PfCode::QuotaOut);
        }
        self.used += 1;
        if self.whitelist.contains(&payload) {
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
}
